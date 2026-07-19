use hyper::StatusCode;

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("no route matched")]
    RouteNotFound,
    #[error("rate limited")]
    RateLimited,
    #[error("authentication failed")]
    AuthFailed,
    #[error("upstream error: {0}")]
    Upstream(String),
    #[error("upstream timeout")]
    Timeout,
}

impl GatewayError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::RouteNotFound => StatusCode::NOT_FOUND,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::AuthFailed => StatusCode::UNAUTHORIZED,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
            Self::Timeout => StatusCode::GATEWAY_TIMEOUT,
        }
    }
}
