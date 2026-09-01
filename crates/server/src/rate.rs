// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Login throttling per name and per address with exponential backoff.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

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

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;
    const KEYS: &[&str] = &["login:mira", "ip:203.0.113.7"];

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
