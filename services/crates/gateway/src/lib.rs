use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioTimer;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use std::{sync::LazyLock, time::Duration};

pub mod config;
pub mod error;
pub mod jwks;
pub mod rate_limiter;
pub mod router;
pub mod service;

pub static HTTP_CLIENT: LazyLock<Client<HttpConnector, BoxBody<Bytes, hyper::Error>>> = LazyLock::new(|| {
    Client::builder(TokioExecutor::new())
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_timer(TokioTimer::new())
        .pool_max_idle_per_host(32)
        .build(HttpConnector::new())
});
