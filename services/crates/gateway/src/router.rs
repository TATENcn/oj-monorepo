use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use futures::future::BoxFuture;
use http_body_util::{BodyExt, combinators::BoxBody};
use hyper::{Request, Response, StatusCode, Uri, body::Incoming, header::HOST};
use tokio::time;
use tower::{Layer, Service};
use tracing::{error, info};

use crate::HTTP_CLIENT;
use crate::config::{MatchType, RouteConfig};
use crate::error::GatewayError;

#[derive(Debug, Clone)]
pub struct RouteMatch {
    pub config: Arc<RouteConfig>,
}

pub fn match_route(routes: &[Arc<RouteConfig>], request_path: &str) -> Option<RouteMatch> {
    routes
        .iter()
        .fold(None, |best: Option<((usize, bool), &Arc<RouteConfig>)>, route| {
            let matches = match route.match_type {
                MatchType::Exact => request_path == route.path,
                MatchType::Prefix => is_prefix_match(&route.path, request_path),
            };
            if !matches {
                return best;
            }
            let is_exact = matches!(route.match_type, MatchType::Exact);
            let key = (route.path.len(), is_exact);
            match best {
                None => Some((key, route)),
                Some((best_key, _)) if key > best_key => Some((key, route)),
                Some(prev) => Some(prev),
            }
        })
        .map(|(_, route)| RouteMatch { config: Arc::clone(route) })
}

fn is_prefix_match(prefix: &str, path: &str) -> bool {
    if !path.starts_with(prefix) {
        return false;
    }

    // Must match a path segment boundary, either exact match or followed by '/'
    path.len() == prefix.len() || path.as_bytes()[prefix.len()] == b'/'
}

#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("no route matched")]
    NoRoute,
    #[error("invalid upstream URI: {0}")]
    InvalidUpstream(String),
    #[error("upstream request timed out")]
    Timeout,
    #[error("upstream error: {0}")]
    Upstream(#[from] hyper_util::client::legacy::Error),
    #[error("failed to read request body: {0}")]
    BodyRead(#[from] hyper::Error),
}

impl ProxyError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::NoRoute => StatusCode::NOT_FOUND,
            Self::InvalidUpstream(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Timeout => StatusCode::GATEWAY_TIMEOUT,
            Self::Upstream(_) | Self::BodyRead(_) => StatusCode::BAD_GATEWAY,
        }
    }
}

/// Proxy an incoming request to the matched upstream
pub async fn proxy(req: Request<Incoming>, route_match: &RouteMatch, timeout_duration: Duration) -> Result<Response<BoxBody<Bytes, hyper::Error>>, ProxyError> {
    let upstream_uri =
        build_upstream_uri(&route_match.config.upstream, req.uri().path(), req.uri().query()).map_err(|e| ProxyError::InvalidUpstream(e.to_string()))?;

    info!(upstream = %upstream_uri, "proxying request");

    let (mut parts, body) = req.into_parts();
    parts.uri = upstream_uri.clone();

    // Replace Host header to match the upstream
    if let Some(authority) = parts.uri.authority() {
        match hyper::header::HeaderValue::from_str(authority.as_str()) {
            Ok(host_val) => {
                parts.headers.insert(HOST, host_val);
            }
            Err(_) => {
                return Err(ProxyError::InvalidUpstream(parts.uri.to_string()));
            }
        }
    }

    let upstream_req = Request::from_parts(parts, body.boxed());
    let res = time::timeout(timeout_duration, HTTP_CLIENT.request(upstream_req))
        .await
        .map_err(|_| {
            error!(upstream = %upstream_uri, "upstream request timed out");
            ProxyError::Timeout
        })?
        .map_err(|e| {
            error!(?e, upstream = %upstream_uri, "upstream request failed");
            ProxyError::Upstream(e)
        })?;

    let (mut parts, body) = res.into_parts();
    // Strip `Content-Length` so hyper recalculates the transfer encoding from
    // the boxed body's size hint (`Content-Length` or chunked, depending on
    // whether the upstream body has a known size)
    parts.headers.remove(hyper::header::CONTENT_LENGTH);

    Ok(Response::from_parts(parts, body.boxed()))
}

pub fn build_upstream_uri(upstream: &str, path: &str, query: Option<&str>) -> Result<Uri, hyper::http::uri::InvalidUri> {
    // Remove trailing slash from upstream uri
    let mut uri = upstream.trim_end_matches('/').to_string();
    uri.push_str(path);

    if let Some(q) = query {
        uri.push('?');
        uri.push_str(q);
    }

    uri.parse()
}

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

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for RouteService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Error: Into<GatewayError>,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
    ResBody: Send + 'static,
{
    type Response = S::Response;
    type Error = GatewayError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        let path = req.uri().path().to_string();

        match match_route(&self.routes, &path) {
            Some(matched) => {
                req.extensions_mut().insert(matched);
                let mut inner = self.inner.clone();
                Box::pin(async move {
                    futures::future::poll_fn(|cx| inner.poll_ready(cx)).await.map_err(Into::into)?;
                    inner.call(req).await.map_err(Into::into)
                })
            }
            None => Box::pin(async { Err(GatewayError::RouteNotFound) }),
        }
    }
}
