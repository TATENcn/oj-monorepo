use std::time::Instant;
use std::{fmt::Debug, future::Future, pin::Pin, task::Context, task::Poll, time::Duration};

use futures::future;
use http_body_util::BodyExt;
use http_body_util::combinators::BoxBody;
use hyper::{Request, Response, body::Incoming};
use tower::Service;
use tracing::{debug, error, info};

use crate::error::GatewayError;
use crate::router::{self, RouteMatch};

pub type HttpBody = BoxBody<bytes::Bytes, hyper::Error>;

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
            Some(m) => m,
            None => {
                error!("route match not found in extensions");
                return Box::pin(async { Err(GatewayError::RouteNotFound) });
            }
        };

        let upstream_uri = match router::build_upstream_uri(&route_match.config.upstream, req.uri().path(), req.uri().query()) {
            Ok(uri) => uri,
            Err(e) => {
                let msg = e.to_string();
                error!(error = %msg, "failed to build upstream URI");
                return Box::pin(async { Err(GatewayError::Upstream(msg)) });
            }
        };

        let (mut parts, body) = req.into_parts();
        parts.uri = upstream_uri.clone();

        if let Some(authority) = parts.uri.authority() {
            if let Ok(host_val) = hyper::header::HeaderValue::from_str(authority.as_str()) {
                parts.headers.insert(hyper::header::HOST, host_val);
            } else {
                return Box::pin(async { Err(GatewayError::Upstream("invalid upstream authority".into())) });
            }
        }

        let upstream_req = Request::from_parts(parts, body);

        info!(%upstream_uri, "proxying request");

        let timeout = self.timeout;
        let mut client = self.client.clone();

        Box::pin(async move {
            let start = Instant::now();
            let res = tokio::time::timeout(timeout, async {
                future::poll_fn(|cx| client.poll_ready(cx)).await?;
                client.call(upstream_req).await
            })
            .await
            .map_err(|_| {
                error!(%upstream_uri, "upstream request timed out");
                GatewayError::Timeout
            })?
            .map_err(|e| {
                error!(%upstream_uri, error = %e, "upstream request failed");
                GatewayError::Upstream(e.to_string())
            })?;

            let (mut parts, body) = res.into_parts();
            let elapsed = start.elapsed();
            debug!(%upstream_uri, duration_ms = elapsed.as_millis(), "upstream responded");
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
