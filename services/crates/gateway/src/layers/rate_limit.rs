use std::net::SocketAddr;
use std::sync::Arc;
use std::task::{Context, Poll};

use hyper::Request;
use tower::{Layer, Service};
use tracing::warn;

use super::{forward, poll_ready};
use crate::error::GatewayError;
use crate::rate_limiter::{self, RateLimiter};
use crate::router::RouteMatch;
use crate::services::HttpBody;

pub struct RateLimitLayer {
    limiter: Arc<dyn RateLimiter>,
}

impl RateLimitLayer {
    pub fn new(limiter: Arc<dyn RateLimiter>) -> Self {
        Self { limiter }
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            inner,
            limiter: self.limiter.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    limiter: Arc<dyn RateLimiter>,
}

impl<S> Service<Request<HttpBody>> for RateLimitService<S>
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

    fn call(&mut self, req: Request<HttpBody>) -> Self::Future {
        let route = match req.extensions().get::<RouteMatch>() {
            Some(r) => r,
            None => return Box::pin(async { Err(GatewayError::RouteNotFound) }),
        };

        let cfg = &route.config.rate_limit;
        let peer_addr = req.extensions().get::<SocketAddr>().copied();

        if let Some(client_ip) = rate_limiter::client_ip(req.headers(), peer_addr) {
            let mut key = String::with_capacity(route.config.path.len() + 1 + client_ip.len());
            key.push_str(&route.config.path);
            key.push(':');
            key.push_str(&client_ip);

            if !self.limiter.check(&key, cfg.per_sec, cfg.burst) {
                warn!(%key, "rate limit exceeded");
                return Box::pin(async { Err(GatewayError::RateLimited) });
            }
        }

        forward!(self.inner, req)
    }
}
