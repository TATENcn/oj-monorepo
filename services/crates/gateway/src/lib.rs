use bytes::Bytes;
use http_body_util::Full;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioTimer;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use std::{sync::LazyLock, time::Duration};

pub mod config;

pub static HTTP_CLIENT: LazyLock<Client<HttpConnector, Full<Bytes>>> = LazyLock::new(|| {
    Client::builder(TokioExecutor::new())
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_timer(TokioTimer::new())
        .pool_max_idle_per_host(32)
        .build(HttpConnector::new())
});
