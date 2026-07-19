use std::sync::Arc;
use std::task::{Context, Poll};

use hyper::Request;
use tower::{Layer, Service};
use tracing::{debug, info};

use super::{forward, poll_ready};
use crate::config::RouteConfig;
use crate::error::GatewayError;
use crate::router::match_route;
use crate::services::HttpBody;

pub struct RouteLayer {
    routes: Arc<Vec<Arc<RouteConfig>>>,
}

impl RouteLayer {
    pub fn new(routes: Arc<Vec<Arc<RouteConfig>>>) -> Self {
        Self { routes }
    }
}

impl<S> Layer<S> for RouteLayer {
    type Service = RouteService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RouteService {
            inner,
            routes: self.routes.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RouteService<S> {
    inner: S,
    routes: Arc<Vec<Arc<RouteConfig>>>,
}

impl<S> Service<Request<HttpBody>> for RouteService<S>
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
        let path = req.uri().path();

        match match_route(&self.routes, path) {
            Some(matched) => {
                debug!(%path, upstream = %matched.config.upstream, "route matched");
                req.extensions_mut().insert(matched);
                forward!(self.inner, req)
            }
            None => {
                info!(%path, "no route matched");
                Box::pin(async { Err(GatewayError::RouteNotFound) })
            }
        }
    }
}
