//! Small in-memory token bucket for API throttling.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::HeaderMap;

use crate::server::auth::extract_bearer;

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

#[derive(Debug)]
pub struct RateLimiter {
    capacity: f64,
    refill_per_second: f64,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimiter {
    pub fn new(requests_per_minute: u32) -> Self {
        let capacity = requests_per_minute.max(1) as f64;
        Self {
            capacity,
            refill_per_second: capacity / 60.0,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    pub fn check(&self, headers: &HeaderMap) -> bool {
        let key = rate_limit_key(headers);
        let now = Instant::now();
        let mut buckets = match self.buckets.lock() {
            Ok(buckets) => buckets,
            Err(_) => return true,
        };

        buckets
            .retain(|_, bucket| now.duration_since(bucket.last_refill) < Duration::from_secs(600));

        let bucket = buckets.entry(key).or_insert_with(|| Bucket {
            tokens: self.capacity,
            last_refill: now,
        });
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            bucket.tokens = (bucket.tokens + elapsed * self.refill_per_second).min(self.capacity);
            bucket.last_refill = now;
        }
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(120)
    }
}

fn rate_limit_key(headers: &HeaderMap) -> String {
    if let Some(token) = extract_bearer(headers) {
        return format!("token:{token}");
    }
    for name in ["x-forwarded-for", "x-real-ip"] {
        if let Some(value) = headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let first = value.split(',').next().unwrap_or(value).trim();
            return format!("ip:{first}");
        }
    }
    "anonymous".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_after_bucket_is_empty() {
        let limiter = RateLimiter::new(2);
        let headers = HeaderMap::new();
        assert!(limiter.check(&headers));
        assert!(limiter.check(&headers));
        assert!(!limiter.check(&headers));
    }
}
