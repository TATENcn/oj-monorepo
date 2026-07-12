use std::{future::Future, net::SocketAddr, pin::Pin, sync::Arc, time::Duration};

use bytes::Bytes;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::{Request, Response, StatusCode, body::Incoming, header};
use serde::Serialize;
use tracing::error;
use uuid::Uuid;

use crate::{
    config::{AuthenticationLevel, RouteConfig},
    jwks::JwksManager,
    rate_limiter::{self, RateLimiter},
    router::{self, ProxyError},
};

type AuthError = Box<Response<BoxBody<Bytes, hyper::Error>>>;

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

pub struct ProxyService {
    routes: Arc<Vec<RouteConfig>>,
    timeout: Duration,
    jwks: Arc<JwksManager>,
    rate_limiter: Arc<dyn RateLimiter>,
}

impl ProxyService {
    pub fn new(routes: Vec<RouteConfig>, timeout: Duration, jwks: JwksManager, rate_limiter: Arc<dyn RateLimiter>) -> Self {
        Self {
            routes: Arc::new(routes),
            timeout,
            jwks: Arc::new(jwks),
            rate_limiter,
        }
    }
}

async fn handle_request(
    req: Request<Incoming>,
    matched: router::RouteMatch<'_>,
    timeout: Duration,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, ProxyError> {
    router::proxy(req, &matched, timeout).await
}

fn into_boxed_body(bytes: Bytes) -> BoxBody<Bytes, hyper::Error> {
    Full::new(bytes).map_err(|e: std::convert::Infallible| match e {}).boxed()
}

impl hyper::service::Service<Request<Incoming>> for ProxyService {
    type Response = Response<BoxBody<Bytes, hyper::Error>>;
    type Error = hyper::http::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let routes = self.routes.clone();
        let timeout = self.timeout;
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

            let matched = match router::match_route(&routes, path) {
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

            let req = match apply_auth(req, &matched.config.auth, &jwks) {
                Ok(req) => req,
                Err(resp) => return Ok(*resp),
            };

            match handle_request(req, matched, timeout).await {
                Ok(resp) => Ok(resp),
                Err(e) => {
                    error!(?e, "proxy error");
                    Ok(error_response(e.status_code(), &e.to_string()))
                }
            }
        })
    }
}

fn parse_user_id(sub: &str) -> Result<hyper::header::HeaderValue, AuthError> {
    Uuid::parse_str(sub)
        .map(|id| id.to_string().parse().expect("UUID string is always a valid header value"))
        .map_err(|_| error_response_boxed(StatusCode::UNAUTHORIZED, "invalid token subject"))
}

fn error_response(status: StatusCode, msg: &str) -> Response<BoxBody<Bytes, hyper::Error>> {
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
