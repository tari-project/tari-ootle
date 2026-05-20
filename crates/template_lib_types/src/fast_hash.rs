//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use core::hash::{BuildHasherDefault, Hasher};

use indexmap::IndexMap;
use xxhash_rust::xxh3::Xxh3Builder;

/// A no-op hasher designed for keys that are already high-quality hashes
/// (e.g. SHA-256 digests, Pedersen commitment bytes, or any 32-byte hash output).
///
/// # How it works
/// XOR-folds the raw key bytes into a single `u64` for use as the HashMap bucket
/// discriminant. This is not cryptographic — it is purely a bucket distribution
/// function. The quality of distribution depends on the entropy already present
/// in the key, which is assumed to be high.
///
/// # Invariants
/// - The key type `K` must have uniformly distributed byte representations. Using this hasher with low-entropy keys
///   (e.g. small integers, sequential IDs) will produce poor bucket distribution and degrade lookup to O(n).
/// - Keys must be at least 8 bytes. Shorter keys will still work but use less entropy for the fold, increasing
///   collision probability.
/// - This hasher provides NO DoS resistance. It must only be used when the caller controls or trusts all inserted keys.
#[derive(Default)]
pub struct PassthroughHasher(u64);

impl Hasher for PassthroughHasher {
    /// XOR-folds `bytes` into a `u64` in 8-byte chunks.
    ///
    /// For a 32-byte key this performs 4 XOR operations, giving good avalanche
    /// across the full key without any multiplication or branching.
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut h = 0u64;
        for chunk in bytes.chunks(8) {
            let mut buf = [0u8; 8];
            buf[..chunk.len()].copy_from_slice(chunk);
            h ^= u64::from_le_bytes(buf);
        }
        self.0 = h;
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

/// General purpose map — safe for any key type, good performance.
///
/// Uses XXH3 as the hasher, which is fast and has excellent distribution
/// for both short and long keys. Suitable when:
/// - Keys are low-entropy (sequential IDs, small integers, short strings)
/// - Key type or entropy level is unknown
/// - DoS resistance is not required but key quality is uncertain
pub type FastMap<K, V> = IndexMap<K, V, Xxh3Builder>;

/// Optimised map for keys that are already high-entropy hash outputs.
///
/// Uses a passthrough XOR-fold hasher, skipping any real hash computation
/// on the assumption that the key already has sufficient entropy for bucket
/// distribution. Suitable when:
/// - Keys are cryptographic hash outputs (SHA-256, Blake2, Poseidon, etc.)
/// - Keys are Pedersen commitments or similar 32-byte hash-typed values
/// - You want to avoid double-hashing an already-hashed key
///
/// # Warning
/// Do NOT use with low-entropy keys — will degrade to O(n) lookup.
/// When in doubt, use [`FastMap`] instead.
pub type PrehashedMap<K, V> = IndexMap<K, V, BuildHasherDefault<PassthroughHasher>>;

/// Adapter that lets `IndexMap<K, V, S>` (including [`FastMap`] and [`PrehashedMap`])
/// participate in minicbor derives via `#[cbor(with = "indexmap_codec")]`.
///
/// On the wire this uses the standard CBOR map type, mirroring the encoding produced
/// by `BTreeMap<K, V>`. On decode, the entries are inserted in iteration order so the
/// resulting `IndexMap` preserves the order encoded by the sender.
///
/// Implementation lives in `tari_bor::adapters::indexmap_codec` so any crate can use it
/// without re-implementing the codec.
pub use tari_bor::adapters::indexmap_codec;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use minicbor::{CborLen, Decode, Encode};

    use super::*;

    #[derive(Debug, PartialEq, Eq, Encode, Decode, CborLen)]
    struct WrapPrehashed(#[cbor(n(0), with = "indexmap_codec")] PrehashedMap<u64, u32>);

    #[derive(Debug, PartialEq, Eq, Encode, Decode, CborLen)]
    struct WrapFast(#[cbor(n(0), with = "indexmap_codec")] FastMap<u32, u32>);

    #[test]
    fn indexmap_codec_round_trips_and_matches_btreemap_wire_format() {
        let mut prehashed: PrehashedMap<u64, u32> = PrehashedMap::default();
        prehashed.insert(1, 100);
        prehashed.insert(2, 200);
        prehashed.insert(3, 300);

        let encoded = tari_bor::encode(&WrapPrehashed(prehashed.clone())).unwrap();
        assert_eq!(encoded.len(), WrapPrehashed(prehashed.clone()).cbor_len(&mut ()));

        // The map portion of the wire bytes must match the canonical BTreeMap encoding —
        // both call `e.map(n)?` followed by alternating k/v.
        let btree: BTreeMap<u64, u32> = prehashed.iter().map(|(k, v)| (*k, *v)).collect();
        let btree_encoded = tari_bor::encode(&btree).unwrap();
        assert!(encoded.ends_with(&btree_encoded));

        let WrapPrehashed(decoded) = tari_bor::decode(&encoded).unwrap();
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[&1], 100);
        assert_eq!(decoded[&2], 200);
        assert_eq!(decoded[&3], 300);
    }

    #[test]
    fn indexmap_codec_preserves_insertion_order_on_decode() {
        let mut m: FastMap<u32, u32> = FastMap::default();
        m.insert(3, 30);
        m.insert(1, 10);
        m.insert(2, 20);

        let encoded = tari_bor::encode(&WrapFast(m.clone())).unwrap();
        let WrapFast(decoded) = tari_bor::decode(&encoded).unwrap();

        let keys_in: Vec<u32> = m.keys().copied().collect();
        let keys_out: Vec<u32> = decoded.keys().copied().collect();
        assert_eq!(keys_in, keys_out, "IndexMap iteration order must round-trip");
    }
}
