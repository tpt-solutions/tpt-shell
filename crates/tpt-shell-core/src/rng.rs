// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `Rng` trait (spec §10.5). Used for ids and jitter, never for secrets: nothing here is a
//! cryptographically secure generator.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

/// Source of pseudo-random 64-bit values.
pub trait Rng: Send {
    fn next_u64(&mut self) -> u64;

    /// A value in `0..bound`. Returns 0 when `bound` is 0.
    fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next_u64() % bound
    }
}

/// SplitMix64. Small, fast, and a fixed sequence for a fixed seed, so it suits tests and replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeededRng {
    state: u64,
}

impl SeededRng {
    pub fn new(seed: u64) -> Self {
        SeededRng { state: seed }
    }
}

impl Rng for SeededRng {
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// A generator seeded from the standard library's per-process random keys.
///
/// The seed is not cryptographically strong. Use it for ids and jitter only.
pub struct SystemRng {
    inner: SeededRng,
}

impl SystemRng {
    pub fn new() -> Self {
        // `RandomState` is seeded from OS randomness once per process.
        let seed = RandomState::new().build_hasher().finish();
        SystemRng {
            inner: SeededRng::new(seed),
        }
    }
}

impl Default for SystemRng {
    fn default() -> Self {
        Self::new()
    }
}

impl Rng for SystemRng {
    fn next_u64(&mut self) -> u64 {
        self.inner.next_u64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_gives_same_sequence() {
        let mut a = SeededRng::new(42);
        let mut b = SeededRng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = SeededRng::new(1);
        let mut b = SeededRng::new(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn below_stays_in_range() {
        let mut rng = SeededRng::new(7);
        for _ in 0..1000 {
            assert!(rng.below(10) < 10);
        }
        assert_eq!(rng.below(0), 0);
    }

    #[test]
    fn system_rng_produces_varied_output() {
        let mut rng = SystemRng::new();
        let first = rng.next_u64();
        assert!((0..16).any(|_| rng.next_u64() != first));
    }
}
