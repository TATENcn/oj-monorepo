use std::{future::Future, pin::Pin};

use bytes::Bytes;
use futures::future;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, StatusCode, body::Incoming};
use tower::Service;
use tracing::error;

use super::proxy::HttpBody;
use crate::error::GatewayError;

fn into_boxed_body(bytes: Bytes) -> HttpBody {
    Full::new(bytes).map_err(|e: std::convert::Infallible| match e {}).boxed()
}

fn error_response(status: StatusCode, msg: &str) -> Response<HttpBody> {
    Response::builder()
        .status(status)
        .body(into_boxed_body(Bytes::copy_from_slice(msg.as_bytes())))
        .expect("building error response")
}

pub struct GatewayService<P> {
    pipeline: P,
}

impl<P> GatewayService<P> {
    pub fn new(pipeline: P) -> Self {
        Self { pipeline }
    }
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
        let mut pipeline = self.pipeline.clone();

        Box::pin(async move {
            let (parts, body) = req.into_parts();
            let collected = match body.collect().await {
                Ok(c) => c,
                Err(e) => {
                    error!(?e, "failed to collect request body");
                    return Ok(error_response(StatusCode::BAD_REQUEST, "failed to read request body"));
                }
            };
            let body_bytes: HttpBody = Full::new(collected.to_bytes()).map_err(|e: std::convert::Infallible| match e {}).boxed();

            let pipeline_req = Request::from_parts(parts, body_bytes);

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
        }
    }
}
