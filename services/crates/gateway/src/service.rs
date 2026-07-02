use std::{future::Future, pin::Pin, time::Duration};

use bytes::Bytes;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::{Request, Response, body::Incoming};
use tracing::error;

use crate::{
    config::RouteConfig,
    router::{self, ProxyError},
};

pub struct ProxyService {
    routes: Vec<RouteConfig>,
    timeout: Duration,
}

impl ProxyService {
    pub fn new(routes: Vec<RouteConfig>, timeout: Duration) -> Self {
        Self { routes, timeout }
    }
}

async fn handle_request(req: Request<Incoming>, routes: &[RouteConfig], timeout: Duration) -> Result<Response<BoxBody<Bytes, hyper::Error>>, ProxyError> {
    let path = req.uri().path().to_string();
    let matched = router::match_route(routes, &path).ok_or(ProxyError::NoRoute)?;
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

        Box::pin(async move {
            match handle_request(req, &routes, timeout).await {
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
