use std::{fmt::Debug, future::Future, net::SocketAddr, pin::Pin, sync::Arc, task::Context, task::Poll, time::Duration};

use bytes::Bytes;
use futures::future;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::{Request, Response, StatusCode, body::Incoming, header};
use serde::Serialize;
use tower::Service;
use tracing::error;
use uuid::Uuid;

use crate::{
    config::{AuthenticationLevel, RouteConfig},
    error::GatewayError,
    jwks::JwksManager,
    rate_limiter::{self, RateLimiter},
    router::{self, RouteMatch},
};

pub type HttpBody = BoxBody<Bytes, hyper::Error>;
type AuthError = Box<Response<HttpBody>>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
enum Health {
    Ok,
    Degraded { reason: &'static str },
}

impl Health {
    fn from_jwks_ready(ready: bool) -> Self {
        if ready { Self::Ok } else { Self::Degraded { reason: "jwks_unavailable" } }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            Self::Ok => StatusCode::OK,
            Self::Degraded { .. } => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

// TODO: That's so ugly, i think we should do something instead of hard-encoding this
impl From<Health> for Bytes {
    fn from(val: Health) -> Self {
        match val {
            Health::Ok => Bytes::from_static(b"{\"status\":\"ok\"}"),
            Health::Degraded { reason } => Bytes::from(format!("{{\"status\":\"degraded\",\"reason\":\"{}\"}}", reason)),
        }
    }
}

// Proxy service
pub struct ProxyService<S> {
    client: S,
    timeout: Duration,
}

impl<S> ProxyService<S> {
    pub fn new(client: S, timeout: Duration) -> Self {
        Self { client, timeout }
    }
}

impl<S> Service<Request<HttpBody>> for ProxyService<S>
where
    S: Service<Request<HttpBody>, Response = Response<Incoming>> + Clone + Send + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
    S::Future: Send,
{
    type Response = Response<HttpBody>;
    type Error = GatewayError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.client.poll_ready(cx).map_err(|e| GatewayError::Upstream(e.to_string()))
    }

    fn call(&mut self, req: Request<HttpBody>) -> Self::Future {
        let route_match = match req.extensions().get::<RouteMatch>() {
            Some(m) => m.clone(),
            None => return Box::pin(async { Err(GatewayError::RouteNotFound) }),
        };

        let upstream_uri = match router::build_upstream_uri(&route_match.config.upstream, req.uri().path(), req.uri().query()) {
            Ok(uri) => uri,
            Err(e) => {
                let msg = e.to_string();
                return Box::pin(async { Err(GatewayError::Upstream(msg)) });
            }
        };

        let (mut parts, body) = req.into_parts();
        parts.uri = upstream_uri.clone();

        // Replace Host header to match the upstream
        if let Some(authority) = parts.uri.authority() {
            if let Ok(host_val) = hyper::header::HeaderValue::from_str(authority.as_str()) {
                parts.headers.insert(hyper::header::HOST, host_val);
            } else {
                return Box::pin(async { Err(GatewayError::Upstream("invalid upstream authority".into())) });
            }
        }

        let upstream_req = Request::from_parts(parts, body);

        let timeout = self.timeout;
        let mut client = self.client.clone();

        Box::pin(async move {
            let res = tokio::time::timeout(timeout, async {
                future::poll_fn(|cx| client.poll_ready(cx)).await?;
                client.call(upstream_req).await
            })
            .await
            .map_err(|_| GatewayError::Timeout)?
            .map_err(|e| GatewayError::Upstream(e.to_string()))?;

            let (mut parts, body) = res.into_parts();
            parts.headers.remove(hyper::header::CONTENT_LENGTH);
            Ok(Response::from_parts(parts, body.boxed()))
        })
    }
}

impl<S> Debug for ProxyService<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyService").field("timeout", &self.timeout).finish_non_exhaustive()
    }
}

impl<S: Clone> Clone for ProxyService<S> {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            timeout: self.timeout,
        }
    }
}

pub struct GatewayService<P> {
    pipeline: P,
    routes: Arc<Vec<Arc<RouteConfig>>>,
    jwks: Arc<JwksManager>,
    rate_limiter: Arc<dyn RateLimiter>,
}

impl<P> GatewayService<P> {
    pub fn new(pipeline: P, routes: Arc<Vec<Arc<RouteConfig>>>, jwks: JwksManager, rate_limiter: Arc<dyn RateLimiter>) -> Self {
        Self {
            pipeline,
            routes: routes,
            jwks: Arc::new(jwks),
            rate_limiter,
        }
    }
}

fn into_boxed_body(bytes: Bytes) -> HttpBody {
    Full::new(bytes).map_err(|e: std::convert::Infallible| match e {}).boxed()
}

impl<P> hyper::service::Service<Request<Incoming>> for GatewayService<P>
where
    P: Service<Request<HttpBody>, Response = Response<HttpBody>, Error = GatewayError> + Clone + Send + 'static,
    P::Future: Send,
{
    type Response = Response<HttpBody>;
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let routes = self.routes.clone();
        let mut pipeline = self.pipeline.clone();
        let jwks = self.jwks.clone();
        let rate_limiter = self.rate_limiter.clone();

        Box::pin(async move {
            let path = req.uri().path();

            // REVIEW: Should we use a configuration field for this?
            // Health check
            if path == "/healthz" {
                let health = Health::from_jwks_ready(jwks.is_ready());
                return Ok(Response::builder()
                    .status(health.status_code())
                    .header("Content-Type", "application/json")
                    .body(into_boxed_body(health.into()))
                    .expect("building healthcheck response"));
            }

            // Route match
            let matched = match router::match_route(&routes, &path) {
                Some(m) => m,
                None => return Ok(error_response(StatusCode::NOT_FOUND, "no route matched")),
            };

            // Rate limit check
            let peer_addr = req.extensions().get::<SocketAddr>().copied();
            if let Some(client_ip) = rate_limiter::client_ip(req.headers(), peer_addr) {
                let cfg = &matched.config.rate_limit;

                let mut key = String::with_capacity(matched.config.path.len() + 1 + client_ip.len());
                key.push_str(&matched.config.path);
                key.push(':');
                key.push_str(&client_ip);

                if !rate_limiter.check(&key, cfg.per_sec, cfg.burst) {
                    return Ok(error_response(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded"));
                }
            }

            // Auth
            let req = match apply_auth(req, &matched.config.auth, &jwks) {
                Ok(req) => req,
                Err(resp) => return Ok(*resp),
            };

            // Collect body for tower pipeline
            let (parts, body) = req.into_parts();
            let collected = match body.collect().await {
                Ok(c) => c,
                Err(e) => {
                    error!(?e, "failed to collect request body");
                    return Ok(error_response(StatusCode::BAD_REQUEST, "failed to read request body"));
                }
            };
            let body_bytes: HttpBody = Full::new(collected.to_bytes()).map_err(|e: std::convert::Infallible| match e {}).boxed();

            let mut pipeline_req = Request::from_parts(parts, body_bytes);
            pipeline_req.extensions_mut().insert(matched);

            match future::poll_fn(|cx| pipeline.poll_ready(cx)).await {
                Ok(()) => {}
                Err(e) => {
                    error!(?e, "pipeline not ready");
                    return Ok(error_response(e.status_code(), &e.to_string()));
                }
            }

            match pipeline.call(pipeline_req).await {
                Ok(resp) => Ok(resp),
                Err(e) => {
                    error!(?e, "pipeline error");
                    Ok(error_response(e.status_code(), &e.to_string()))
                }
            }
        })
    }
}

impl<P: Clone> Clone for GatewayService<P> {
    fn clone(&self) -> Self {
        Self {
            pipeline: self.pipeline.clone(),
            routes: self.routes.clone(),
            jwks: self.jwks.clone(),
            rate_limiter: self.rate_limiter.clone(),
        }
    }
}

fn parse_user_id(sub: &str) -> Result<hyper::header::HeaderValue, AuthError> {
    Uuid::parse_str(sub)
        .map(|id| id.to_string().parse().expect("UUID string is always a valid header value"))
        .map_err(|_| error_response_boxed(StatusCode::UNAUTHORIZED, "invalid token subject"))
}

fn error_response(status: StatusCode, msg: &str) -> Response<HttpBody> {
    Response::builder()
        .status(status)
        .body(into_boxed_body(Bytes::copy_from_slice(msg.as_bytes())))
        .expect("building error response")
}

/// Apply authentication based on the route's level
fn apply_auth(mut req: Request<Incoming>, level: &AuthenticationLevel, jwks: &JwksManager) -> Result<Request<Incoming>, AuthError> {
    match level {
        AuthenticationLevel::None => Ok(req),
        AuthenticationLevel::BypassAndStrip => {
            req.headers_mut().remove(header::AUTHORIZATION);
            Ok(req)
        }
        AuthenticationLevel::Required => {
            let claims = verify_token(req.headers().get(header::AUTHORIZATION), jwks)?;
            let user_id = parse_user_id(&claims.sub)?;
            req.headers_mut().insert(header::HeaderName::from_static("x-user-id"), user_id);
            Ok(req)
        }
        AuthenticationLevel::Optional => {
            if let Some(val) = req.headers().get(header::AUTHORIZATION) {
                let claims = verify_token(Some(val), jwks)?;
                let user_id = parse_user_id(&claims.sub)?;
                req.headers_mut().insert(header::HeaderName::from_static("x-user-id"), user_id);
            }
            Ok(req)
        }
    }
}

fn verify_token(auth_header: Option<&hyper::header::HeaderValue>, jwks: &JwksManager) -> Result<auth::token::Claims, AuthError> {
    let header_val = auth_header.ok_or_else(|| error_response_boxed(StatusCode::UNAUTHORIZED, "missing authorization header"))?;

    let header_str = header_val
        .to_str()
        .map_err(|_| error_response_boxed(StatusCode::UNAUTHORIZED, "invalid authorization header"))?;

    let token = header_str
        .strip_prefix("Bearer ")
        .ok_or_else(|| error_response_boxed(StatusCode::UNAUTHORIZED, "invalid authorization header"))?;

    jwks.verify(token).map_err(|e| {
        error!(?e, "JWT verification failed");
        error_response_boxed(StatusCode::UNAUTHORIZED, &e.to_string())
    })
}

fn error_response_boxed(status: StatusCode, msg: &str) -> AuthError {
    Box::new(error_response(status, msg))
}
