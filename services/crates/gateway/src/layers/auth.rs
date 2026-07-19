use std::sync::Arc;
use std::task::{Context, Poll};

use hyper::{Request, header};
use tower::{Layer, Service};
use tracing::{error, info, warn};
use uuid::Uuid;

use super::{forward, poll_ready};
use crate::config::AuthenticationLevel;
use crate::error::GatewayError;
use crate::jwks::JwksManager;
use crate::router::RouteMatch;
use crate::services::HttpBody;

pub struct AuthLayer {
    jwks: Arc<JwksManager>,
}

impl AuthLayer {
    pub fn new(jwks: Arc<JwksManager>) -> Self {
        Self { jwks }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthService {
            inner,
            jwks: self.jwks.clone(),
        }
    }
}

#[derive(Clone)]
pub struct AuthService<S> {
    inner: S,
    jwks: Arc<JwksManager>,
}

impl<S> Service<Request<HttpBody>> for AuthService<S>
where
    S: Service<Request<HttpBody>, Response = hyper::Response<HttpBody>> + Clone + Send + 'static,
    S::Error: Into<GatewayError>,
    S::Future: Send,
{
    type Response = S::Response;
    type Error = GatewayError;
    type Future = futures::future::BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        poll_ready(&mut self.inner, cx)
    }

    fn call(&mut self, mut req: Request<HttpBody>) -> Self::Future {
        let route = match req.extensions().get::<RouteMatch>() {
            Some(r) => r,
            None => return Box::pin(async { Err(GatewayError::RouteNotFound) }),
        };

        match &route.config.auth {
            AuthenticationLevel::None => {}
            AuthenticationLevel::BypassAndStrip => {
                req.headers_mut().remove(header::AUTHORIZATION);
            }
            AuthenticationLevel::Required => match verify_token(req.headers().get(header::AUTHORIZATION), &self.jwks) {
                Ok(user_id) => {
                    info!(%user_id, "auth succeeded");
                    req.headers_mut().insert(
                        header::HeaderName::from_static("x-user-id"),
                        user_id.parse().expect("UUID string is a valid header value"),
                    );
                }
                Err(()) => {
                    warn!("auth failed");
                    return Box::pin(async { Err(GatewayError::AuthFailed) });
                }
            },
            AuthenticationLevel::Optional => {
                if let Some(val) = req.headers().get(header::AUTHORIZATION) {
                    if let Ok(user_id) = verify_token(Some(val), &self.jwks) {
                        req.headers_mut().insert(
                            header::HeaderName::from_static("x-user-id"),
                            user_id.parse().expect("UUID string is a valid header value"),
                        );
                    }
                }
            }
        }

        forward!(self.inner, req)
    }
}

fn verify_token(auth_header: Option<&hyper::header::HeaderValue>, jwks: &JwksManager) -> Result<String, ()> {
    let header_val = auth_header.ok_or(())?;
    let header_str = header_val.to_str().map_err(|_| ())?;
    let token = header_str.strip_prefix("Bearer ").ok_or(())?;

    let claims = jwks.verify(token).map_err(|e| {
        error!(?e, "JWT verification failed");
    })?;

    Uuid::parse_str(&claims.sub).map(|id| id.to_string()).map_err(|_| ())
}
