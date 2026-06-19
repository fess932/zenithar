//! Tiny in-memory token-bucket rate limiting (Phase 7 abuse control). Good
//! enough for single-node self-hosting — not distributed. Two flavors:
//!   * [`RateLimiter`] — keyed (per principal / per IP), shared via `AppState`.
//!   * [`LocalBucket`] — a single bucket owned by one task (e.g. a WS socket),
//!     so the hot message path needs no lock or map.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Refill a bucket toward `capacity` at `refill`/sec and try to spend one token.
fn spend(tokens: &mut f64, last: &mut Instant, capacity: f64, refill: f64) -> bool {
    let now = Instant::now();
    let elapsed = now.duration_since(*last).as_secs_f64();
    *last = now;
    *tokens = (*tokens + elapsed * refill).min(capacity);
    if *tokens >= 1.0 {
        *tokens -= 1.0;
        true
    } else {
        false
    }
}

struct Bucket {
    tokens: f64,
    last: Instant,
}

/// Keyed limiter: one bucket per key, created lazily. Old buckets are swept
/// opportunistically so memory stays bounded under churn (e.g. many IPs).
pub struct RateLimiter {
    capacity: f64,
    refill: f64,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimiter {
    /// `capacity` = burst size; `refill` = tokens added per second.
    pub fn new(capacity: f64, refill: f64) -> Self {
        Self {
            capacity,
            refill,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Try to consume one token for `key`. `true` = allowed, `false` = limited.
    pub fn check(&self, key: &str) -> bool {
        let mut map = self.buckets.lock().unwrap();
        if map.len() > 4096 {
            let now = Instant::now();
            map.retain(|_, b| now.duration_since(b.last).as_secs() < 300);
        }
        let b = map.entry(key.to_string()).or_insert(Bucket {
            tokens: self.capacity,
            last: Instant::now(),
        });
        spend(&mut b.tokens, &mut b.last, self.capacity, self.refill)
    }
}

/// A single token bucket owned by one task (no sharing, no lock).
pub struct LocalBucket {
    tokens: f64,
    last: Instant,
    capacity: f64,
    refill: f64,
}

impl LocalBucket {
    pub fn new(capacity: f64, refill: f64) -> Self {
        Self {
            tokens: capacity,
            last: Instant::now(),
            capacity,
            refill,
        }
    }

    pub fn check(&mut self) -> bool {
        spend(&mut self.tokens, &mut self.last, self.capacity, self.refill)
    }
}

/// Shared limiters carried in `AppState`.
pub struct Limits {
    /// `GET /i/:token` attempts, keyed by client IP (anti brute-force).
    pub login: RateLimiter,
    /// Uploads, keyed by principal id.
    pub uploads: RateLimiter,
    /// REST API requests, keyed by the (hashed) API token.
    pub api: RateLimiter,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            // 20 link attempts burst, +1/3s sustained.
            login: RateLimiter::new(20.0, 1.0 / 3.0),
            // 30 uploads burst, +1/2s sustained.
            uploads: RateLimiter::new(30.0, 0.5),
            // 60 API calls burst, +5/s sustained (per token).
            api: RateLimiter::new(60.0, 5.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_bucket_limits_then_refills() {
        // Capacity 3, very slow refill so the burst is what we test.
        let mut b = LocalBucket::new(3.0, 0.0);
        assert!(b.check());
        assert!(b.check());
        assert!(b.check());
        assert!(!b.check(), "4th in a burst of 3 is blocked");
    }

    #[test]
    fn keyed_limiter_is_per_key() {
        let rl = RateLimiter::new(1.0, 0.0);
        assert!(rl.check("a"));
        assert!(!rl.check("a"), "second 'a' blocked");
        assert!(rl.check("b"), "different key has its own bucket");
    }
}
