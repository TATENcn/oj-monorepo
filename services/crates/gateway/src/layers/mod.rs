pub mod auth;
pub mod health;
pub mod rate_limit;
pub mod route;

use std::task::{Context, Poll};

use tower::Service;

pub(crate) fn poll_ready<S, Req, E>(inner: &mut S, cx: &mut Context<'_>) -> Poll<Result<(), E>>
where
    S: Service<Req>,
    S::Error: Into<E>,
{
    inner.poll_ready(cx).map_err(Into::into)
}

macro_rules! forward {
    ($inner:expr, $req:expr) => {{
        let mut inner = $inner.clone();
        Box::pin(async move {
            ::futures::future::poll_fn(|cx| ::tower::Service::poll_ready(&mut inner, cx))
                .await
                .map_err(Into::into)?;
            inner.call($req).await.map_err(Into::into)
        })
    }};
}
pub(super) use forward;
