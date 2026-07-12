pub mod memory;

/// Check whether a request is allowed
pub trait RateLimiter: Send + Sync {
    fn check(&self, key: &str, per_sec: u64, burst: u64) -> bool;
}

/// Extract the client IP from `X-Forwarded-For` or fall back to the TCP peer address.
/// Returns `None` when neither is available.
pub fn client_ip(headers: &hyper::HeaderMap, peer_addr: Option<std::net::SocketAddr>) -> Option<String> {
    // X-Forwarded-For: client, proxy1, proxy2
    if let Some(forwarded) = headers.get("x-forwarded-for")
        && let Ok(val) = forwarded.to_str()
        && let Some(ip) = val.split(',').next().map(|s| s.trim())
        && !ip.is_empty()
    {
        return Some(ip.to_string());
    }

    // Fallback to TCP peer address
    peer_addr.map(|a| a.ip().to_string())
}
