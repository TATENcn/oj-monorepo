use std::sync::Arc;
use std::sync::LazyLock;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures::future::BoxFuture;
use http::header;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::{Request, Response, StatusCode};
use serde::Serialize;
use tower::{Layer, Service};
use tracing::debug;

use crate::error::GatewayError;
use crate::jwks::JwksManager;

use super::forward;
use super::poll_ready;

type HttpBody = BoxBody<Bytes, hyper::Error>;

fn into_boxed_body(bytes: Bytes) -> HttpBody {
    Full::new(bytes).map_err(|e: std::convert::Infallible| match e {}).boxed()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum DegradedReason {
    JwksUnavailable,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
enum Health {
    Ok,
    Degraded { reason: DegradedReason },
}

static HEALTH_OK: LazyLock<Bytes> = LazyLock::new(|| serde_json::to_vec(&Health::Ok).expect("Health::Ok serialize").into());
static HEALTH_DEGRADED_JWKS: LazyLock<Bytes> = LazyLock::new(|| {
    serde_json::to_vec(&Health::Degraded {
        reason: DegradedReason::JwksUnavailable,
    })
    .expect("Health::Degraded serialize")
    .into()
});

impl Health {
    fn from_jwks_ready(ready: bool) -> Self {
        if ready {
            Self::Ok
        } else {
            Self::Degraded {
                reason: DegradedReason::JwksUnavailable,
            }
        }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            Self::Ok => StatusCode::OK,
            Self::Degraded { .. } => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

impl From<Health> for Bytes {
    fn from(val: Health) -> Self {
        match val {
            Health::Ok => HEALTH_OK.clone(),
            Health::Degraded { reason } => match reason {
                DegradedReason::JwksUnavailable => HEALTH_DEGRADED_JWKS.clone(),
            },
        }
    }
}

pub struct HealthLayer {
    jwks: Arc<JwksManager>,
}

impl HealthLayer {
    pub fn new(jwks: Arc<JwksManager>) -> Self {
        Self { jwks }
    }
}

impl<S> Layer<S> for HealthLayer {
    type Service = HealthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HealthService {
            inner,
            jwks: self.jwks.clone(),
        }
    }
}

#[derive(Clone)]
pub struct HealthService<S> {
    inner: S,
    jwks: Arc<JwksManager>,
}

impl<S> Service<Request<HttpBody>> for HealthService<S>
where
    S: Service<Request<HttpBody>, Response = Response<HttpBody>> + Clone + Send + 'static,
    S::Error: Into<GatewayError>,
    S::Future: Send,
{
    type Response = Response<HttpBody>;
    type Error = GatewayError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        poll_ready(&mut self.inner, cx)
    }

    fn call(&mut self, req: Request<HttpBody>) -> Self::Future {
        if req.uri().path() == "/healthz" {
            let health = Health::from_jwks_ready(self.jwks.is_ready());
            debug!(status = ?health, "health check");
            let response = Response::builder()
                .status(health.status_code())
                .header(header::CONTENT_TYPE, "application/json")
                .body(into_boxed_body(health.into()))
                .expect("building health check response");
            return Box::pin(async { Ok(response) });
        }

        forward!(self.inner, req)
    }
}
