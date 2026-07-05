//! Seeded, deterministic randomness. Every stochastic effect owns a `Prng`
//! seeded from its config, so the same seed and the same input always
//! produce the same performance.

/// The engine-wide PRNG: small, fast, no allocation, reproducible.
pub type Prng = rand_pcg::Pcg64Mcg;

/// Build a stream from a user seed. Effects that need several independent
/// streams derive them with distinct `stream` values.
pub fn seeded(seed: u64, stream: u64) -> Prng {
    // SplitMix64 over (seed, stream) so nearby seeds produce unrelated
    // states.
    let mut z = seed ^ stream.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut next = || {
        z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut x = z;
        x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^ (x >> 31)
    };
    let state = ((next() as u128) << 64) | next() as u128;
    Prng::new(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;

    #[test]
    fn same_seed_same_stream() {
        let mut a = seeded(42, 0);
        let mut b = seeded(42, 0);
        for _ in 0..100 {
            assert_eq!(a.random::<u64>(), b.random::<u64>());
        }
    }

    #[test]
    fn different_streams_diverge() {
        let mut a = seeded(42, 0);
        let mut b = seeded(42, 1);
        let same = (0..100)
            .filter(|_| a.random::<u64>() == b.random::<u64>())
            .count();
        assert_eq!(same, 0);
    }
}
