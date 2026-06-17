//! A tiny deterministic PRNG (SplitMix64) so simulations are reproducible from a seed.
//!
//! Not cryptographic — it exists only to make generated traffic deterministic and repeatable
//! across runs (same seed → same stream), which matters for evaluation and debugging.

/// A seedable SplitMix64 generator.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Create a generator from a seed.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Next 64-bit value.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `[0, n)`. Panics if `n == 0`.
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "Rng::below requires n > 0");
        // Modulo bias is negligible for simulation-scale ranges.
        self.next_u64() % n
    }

    /// An `i64` in `[lo, hi)`. Panics if `hi <= lo`.
    pub fn range_i64(&mut self, lo: i64, hi: i64) -> i64 {
        assert!(hi > lo, "Rng::range_i64 requires hi > lo");
        let span = (hi - lo) as u64;
        lo + self.below(span) as i64
    }

    /// Pick a reference to a uniformly-random element. Panics on an empty slice.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        assert!(!items.is_empty(), "Rng::pick requires a non-empty slice");
        let idx = self.below(items.len() as u64) as usize;
        &items[idx]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn below_is_in_range() {
        let mut r = Rng::new(7);
        for _ in 0..1000 {
            assert!(r.below(10) < 10);
        }
    }

    #[test]
    fn range_i64_is_within_bounds() {
        let mut r = Rng::new(9);
        for _ in 0..1000 {
            let v = r.range_i64(-5, 5);
            assert!((-5..5).contains(&v));
        }
    }
}
