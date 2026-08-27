//! In-process token buckets keyed by client IP (anonymous routes) or user id.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Bucket {
    tokens: f64,
    last: Instant,
}

pub struct RateLimiter {
    capacity: f64,
    per_second: f64,
    buckets: Mutex<HashMap<String, Bucket>>,
    calls: Mutex<u64>,
}

impl RateLimiter {
    pub fn new(per_minute: u32) -> Self {
        let capacity = per_minute.max(1) as f64;
        Self {
            capacity,
            per_second: capacity / 60.0,
            buckets: Mutex::new(HashMap::new()),
            calls: Mutex::new(0),
        }
    }

    /// Returns true when the key still has budget for one more request.
    pub fn allow(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let bucket = buckets.entry(key.to_string()).or_insert(Bucket {
            tokens: self.capacity,
            last: now,
        });
        let elapsed = now.duration_since(bucket.last).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.per_second).min(self.capacity);
        bucket.last = now;
        let allowed = if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        };
        drop(buckets);
        self.maybe_sweep(now);
        allowed
    }

    fn maybe_sweep(&self, now: Instant) {
        let mut calls = self.calls.lock().unwrap_or_else(|e| e.into_inner());
        *calls += 1;
        if !calls.is_multiple_of(1000) {
            return;
        }
        drop(calls);
        let idle = Duration::from_secs(180);
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        buckets.retain(|_, b| now.duration_since(b.last) < idle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_drains_and_refills() {
        let limiter = RateLimiter::new(60);
        for _ in 0..60 {
            assert!(limiter.allow("k"));
        }
        assert!(!limiter.allow("k"));
        assert!(limiter.allow("other"));
    }
}
