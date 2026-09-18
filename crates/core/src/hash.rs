//! Content hashing for the reconcile manifest (`SPEC.md` §6.3).

use sha2::{Digest, Sha256};
use std::fmt::Write as _;

/// The hex SHA-256 of `bytes`.
#[must_use]
pub fn content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let capacity = digest.len().saturating_mul(2);
    let mut hex = String::with_capacity(capacity);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_is_the_known_sha256() {
        assert_eq!(
            content_hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
