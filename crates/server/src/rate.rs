// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Rate limits kept in memory: login throttling per name and per address
//! with exponential backoff beside a refilling allowance per key.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::session::MS_PER_MINUTE;

/// Failures tolerated per key before delays start; typos stay painless.
const FREE_FAILURES: u32 = 3;

/// First delay once the free run is spent.
const INITIAL_DELAY_MS: i64 = 2_000;

/// Delays stop doubling here; a lock this long already defeats guessing.
const MAX_DELAY_MS: i64 = 900_000;

/// A quiet hour clears a key's history.
const FORGET_AFTER_MS: i64 = 3_600_000;

/// Past this many tracked keys, stale entries are swept on the next write.
const SWEEP_THRESHOLD: usize = 10_000;

struct Entry {
    failures: u32,
    last_failure_at: i64,
    blocked_until: i64,
}

/// In-memory failure tracker; state resets with the process on purpose,
/// since the persistent stop for upstream accounts is a separate rule.
#[derive(Default)]
pub struct RateLimiter {
    entries: Mutex<HashMap<String, Entry>>,
}

impl RateLimiter {
    /// Remaining block in milliseconds when any key is currently held.
    pub fn blocked_for(&self, keys: &[&str], now: i64) -> Option<i64> {
        let entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        keys.iter()
            .filter_map(|key| entries.get(*key))
            .map(|entry| entry.blocked_until.saturating_sub(now))
            .filter(|remaining| *remaining > 0)
            .max()
    }

    /// Records a failed attempt on every key, growing each key's delay.
    pub fn record_failure(&self, keys: &[&str], now: i64) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.len() >= SWEEP_THRESHOLD {
            entries.retain(|_, entry| now.saturating_sub(entry.last_failure_at) < FORGET_AFTER_MS);
        }
        for key in keys {
            let entry = entries.entry((*key).to_owned()).or_insert(Entry {
                failures: 0,
                last_failure_at: now,
                blocked_until: 0,
            });
            if now.saturating_sub(entry.last_failure_at) >= FORGET_AFTER_MS {
                entry.failures = 0;
            }
            entry.failures = entry.failures.saturating_add(1);
            entry.last_failure_at = now;
            entry.blocked_until = now.saturating_add(delay_after(entry.failures));
        }
    }

    /// Clears every key after a successful attempt.
    pub fn record_success(&self, keys: &[&str]) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        for key in keys {
            entries.remove(*key);
        }
    }
}

fn delay_after(failures: u32) -> i64 {
    let Some(beyond_free) = failures.checked_sub(FREE_FAILURES + 1) else {
        return 0;
    };
    INITIAL_DELAY_MS
        .checked_shl(beyond_free)
        .map_or(MAX_DELAY_MS, |delay| delay.min(MAX_DELAY_MS))
}

/// A refilling allowance: `burst` tokens at most, `per_minute` of them
/// back over a minute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    pub burst: u32,
    pub per_minute: u32,
}

impl Rate {
    /// Milliseconds one token takes to come back, one at least, so a
    /// rate faster than the clock still refills without a division by
    /// zero; a rate of zero per minute refills as one a minute.
    fn ms_per_token(self) -> i64 {
        (MS_PER_MINUTE / i64::from(self.per_minute.max(1))).max(1)
    }
}

struct Bucket {
    tokens: i64,
    /// When the bucket last gained a token; the time since then is not
    /// lost to a take.
    refilled_at: i64,
}

impl Bucket {
    /// Tokens gained since the last refill.
    fn gained(&self, ms_per_token: i64, now: i64) -> i64 {
        now.saturating_sub(self.refilled_at).max(0) / ms_per_token
    }

    fn refill(&mut self, burst: i64, ms_per_token: i64, now: i64) {
        let gained = self.gained(ms_per_token, now);
        if gained > 0 {
            self.tokens = self.tokens.saturating_add(gained).min(burst);
            self.refilled_at = self
                .refilled_at
                .saturating_add(gained.saturating_mul(ms_per_token));
        }
    }
}

/// One bucket per key, full when first seen; a bucket back to full is
/// swept once the map grows.
pub struct Buckets {
    rate: Rate,
    entries: Mutex<HashMap<String, Bucket>>,
}

impl Buckets {
    #[must_use]
    pub fn new(rate: Rate) -> Self {
        Self {
            rate,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// One token from the key's bucket at `now`.
    ///
    /// # Errors
    ///
    /// Returns the milliseconds until the next token when the bucket is
    /// empty.
    pub fn take(&self, key: &str, now: i64) -> Result<(), i64> {
        let ms_per_token = self.rate.ms_per_token();
        let burst = i64::from(self.rate.burst);
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.len() >= SWEEP_THRESHOLD {
            entries.retain(|_, bucket| {
                bucket
                    .tokens
                    .saturating_add(bucket.gained(ms_per_token, now))
                    < burst
            });
        }
        let bucket = entries.entry(key.to_owned()).or_insert(Bucket {
            tokens: burst,
            refilled_at: now,
        });
        bucket.refill(burst, ms_per_token, now);
        if bucket.tokens <= 0 {
            let next = bucket.refilled_at.saturating_add(ms_per_token);
            return Err(next.saturating_sub(now).max(1));
        }
        bucket.tokens -= 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;
    const KEYS: &[&str] = &["login:mira", "ip:203.0.113.7"];

    /// Four tokens at once, one back every ten seconds.
    const RATE: Rate = Rate {
        burst: 4,
        per_minute: 6,
    };

    #[test]
    fn a_bucket_holds_its_burst_and_names_the_wait_past_it() {
        let buckets = Buckets::new(RATE);
        for _ in 0..RATE.burst {
            assert_eq!(buckets.take("s1", NOW), Ok(()));
        }
        assert_eq!(buckets.take("s1", NOW), Err(10_000));
        assert_eq!(buckets.take("s1", NOW + 9_999), Err(1));
        assert_eq!(buckets.take("s2", NOW), Ok(()));
    }

    #[test]
    fn a_rate_faster_than_the_clock_still_counts_and_a_rate_of_zero_refills_one_a_minute() {
        let fast = Buckets::new(Rate {
            burst: 1,
            per_minute: 100_000,
        });
        assert_eq!(fast.take("s1", NOW), Ok(()));
        assert_eq!(fast.take("s1", NOW), Err(1));
        assert_eq!(fast.take("s1", NOW + 1), Ok(()));
        let still = Buckets::new(Rate {
            burst: 1,
            per_minute: 0,
        });
        assert_eq!(still.take("s1", NOW), Ok(()));
        assert_eq!(still.take("s1", NOW), Err(MS_PER_MINUTE));
    }

    #[test]
    fn tokens_come_back_at_the_rate_and_never_past_the_burst() {
        let buckets = Buckets::new(RATE);
        for _ in 0..RATE.burst {
            buckets.take("s1", NOW).unwrap();
        }
        assert_eq!(buckets.take("s1", NOW + 10_000), Ok(()));
        assert_eq!(buckets.take("s1", NOW + 10_000), Err(10_000));
        assert_eq!(buckets.take("s1", NOW + 25_000), Ok(()));
        assert_eq!(buckets.take("s1", NOW + 25_000), Err(5_000));
        let hour = NOW + 3_600_000;
        for _ in 0..RATE.burst {
            assert_eq!(buckets.take("s1", hour), Ok(()));
        }
        assert_eq!(buckets.take("s1", hour), Err(10_000));
        assert_eq!(buckets.take("s1", hour - 1), Err(10_001));
    }

    #[test]
    fn the_free_run_carries_no_delay() {
        let limiter = RateLimiter::default();
        for _ in 0..FREE_FAILURES {
            limiter.record_failure(KEYS, NOW);
            assert_eq!(limiter.blocked_for(KEYS, NOW), None);
        }
    }

    #[test]
    fn delays_double_from_the_first_block_and_cap() {
        assert_eq!(delay_after(FREE_FAILURES), 0);
        assert_eq!(delay_after(FREE_FAILURES + 1), INITIAL_DELAY_MS);
        assert_eq!(delay_after(FREE_FAILURES + 2), INITIAL_DELAY_MS * 2);
        assert_eq!(delay_after(u32::MAX), MAX_DELAY_MS);
    }

    #[test]
    fn a_block_holds_either_key_alone_until_it_lapses() {
        let limiter = RateLimiter::default();
        for _ in 0..=FREE_FAILURES {
            limiter.record_failure(KEYS, NOW);
        }
        assert_eq!(limiter.blocked_for(&[KEYS[0]], NOW), Some(INITIAL_DELAY_MS));
        assert_eq!(limiter.blocked_for(&[KEYS[1]], NOW), Some(INITIAL_DELAY_MS));
        assert_eq!(limiter.blocked_for(KEYS, NOW + INITIAL_DELAY_MS), None);
    }

    #[test]
    fn success_clears_the_keys() {
        let limiter = RateLimiter::default();
        for _ in 0..=FREE_FAILURES {
            limiter.record_failure(KEYS, NOW);
        }
        limiter.record_success(KEYS);
        assert_eq!(limiter.blocked_for(KEYS, NOW), None);
    }

    #[test]
    fn a_quiet_hour_resets_the_count() {
        let limiter = RateLimiter::default();
        for _ in 0..=FREE_FAILURES {
            limiter.record_failure(KEYS, NOW);
        }
        let later = NOW + FORGET_AFTER_MS;
        limiter.record_failure(KEYS, later);
        assert_eq!(limiter.blocked_for(KEYS, later), None);
    }
}
