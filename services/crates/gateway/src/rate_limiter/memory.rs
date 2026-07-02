use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use governor::{DefaultDirectRateLimiter, Quota};
use moka::sync::Cache;
use tracing::trace;

use super::RateLimiter;

pub struct InMemoryRateLimiter {
    limiters: Cache<String, Arc<DefaultDirectRateLimiter>>,
}

impl InMemoryRateLimiter {
    pub fn new(eviction_ttl: Duration) -> Self {
        Self {
            limiters: Cache::builder().time_to_idle(eviction_ttl).build(),
        }
    }
}

impl RateLimiter for InMemoryRateLimiter {
    fn check(&self, key: &str, per_sec: u64, burst: u64) -> bool {
        let limiter = self.limiters.get(key).unwrap_or_else(|| {
            let qps = NonZeroU32::new(per_sec as u32).expect("per_sec validated at startup");
            let burst_nz = NonZeroU32::new(burst as u32).expect("burst validated at startup");
            let limiter = Arc::new(DefaultDirectRateLimiter::direct(Quota::per_second(qps).allow_burst(burst_nz)));
            self.limiters.insert(key.to_string(), Arc::clone(&limiter));
            limiter
        });

        let allowed = limiter.check().is_ok();
        trace!(key, status = if allowed { "allowed" } else { "denied" }, "rate limit check");
        allowed
    }
}
