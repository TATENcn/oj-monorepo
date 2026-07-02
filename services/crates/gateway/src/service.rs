use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use bytes::Bytes;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::{Request, Response, StatusCode, body::Incoming, header};
use tracing::error;

use crate::{
    config::{AuthenticationLevel, RouteConfig},
    jwks::JwksManager,
    rate_limiter::{self, RateLimiter},
    router::{self, ProxyError},
};

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
    Full::new(bytes).map_err(|_| unreachable!()).boxed()
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
            let path = req.uri().path().to_string();

            let matched = match router::match_route(&routes, &path) {
                Some(m) => m,
                None => return Ok(error_response(StatusCode::NOT_FOUND, "no route matched")),
            };

            // Rate limit check
            if let Some(client_ip) = rate_limiter::client_ip(req.headers(), None) {
                let cfg = &matched.config.rate_limit;
                let key = format!("{}:{}", matched.config.path, client_ip);
                if !rate_limiter.check(&key, cfg.per_sec, cfg.burst) {
                    return Ok(error_response(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded"));
                }
            }

            let req = match apply_auth(req, &matched.config.auth, &jwks) {
                Ok(req) => req,
                Err(resp) => return Ok(resp),
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

fn error_response(status: StatusCode, msg: &str) -> Response<BoxBody<Bytes, hyper::Error>> {
    Response::builder()
        .status(status)
        .body(into_boxed_body(Bytes::from(msg.to_string())))
        .expect("building error response")
}

/// Apply authentication based on the route's level
fn apply_auth(
    mut req: Request<Incoming>,
    level: &AuthenticationLevel,
    jwks: &JwksManager,
) -> Result<Request<Incoming>, Response<BoxBody<Bytes, hyper::Error>>> {
    match level {
        AuthenticationLevel::None => Ok(req),
        AuthenticationLevel::BypassAndStrip => {
            req.headers_mut().remove(header::AUTHORIZATION);
            Ok(req)
        }
        AuthenticationLevel::Required => {
            let claims = verify_token(req.headers().get(header::AUTHORIZATION), jwks)?;
            req.headers_mut()
                .insert(header::HeaderName::from_static("x-user-id"), claims.sub.parse().expect("sub is valid"));
            Ok(req)
        }
        AuthenticationLevel::Optional => {
            if let Some(val) = req.headers().get(header::AUTHORIZATION) {
                let claims = verify_token(Some(val), jwks)?;

                req.headers_mut()
                    .insert(header::HeaderName::from_static("x-user-id"), claims.sub.parse().expect("sub is valid"));
            }
            Ok(req)
        }
    }
}

fn verify_token(auth_header: Option<&hyper::header::HeaderValue>, jwks: &JwksManager) -> Result<auth::token::Claims, Response<BoxBody<Bytes, hyper::Error>>> {
    let header_val = auth_header.ok_or_else(|| error_response(StatusCode::UNAUTHORIZED, "missing authorization header"))?;

    let header_str = header_val
        .to_str()
        .map_err(|_| error_response(StatusCode::UNAUTHORIZED, "invalid authorization header"))?;

    let token = header_str
        .strip_prefix("Bearer ")
        .ok_or_else(|| error_response(StatusCode::UNAUTHORIZED, "invalid authorization header"))?;

    jwks.verify(token).map_err(|e| {
        error!(?e, "JWT verification failed");
        error_response(StatusCode::UNAUTHORIZED, &e.to_string())
    })
}
