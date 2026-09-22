//! Content hashing for the reconcile manifest (`SPEC.md` §6.3) and for the
//! integrity of every published generation artifact.
//!
//! BLAKE3: its speed comes from generic SIMD with runtime dispatch (NEON on
//! AArch64/Graviton, SSE4.1/AVX2/AVX-512 on x86), not from dedicated SHA
//! instructions that some deployment CPUs lack. Queries verify each committed
//! artifact they read, so the hash is on the query path.

/// The lowercase hex BLAKE3 digest (256-bit) of `bytes`.
#[must_use]
pub fn content_hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// The raw BLAKE3 digest of `bytes`, for fixed-width binary keys.
#[must_use]
pub fn digest(bytes: &[u8]) -> [u8; 32] {
    *blake3::hash(bytes).as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_is_the_known_blake3() {
        assert_eq!(
            content_hash(b"abc"),
            "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
        );
    }
}
