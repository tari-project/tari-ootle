//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::Read;

use anyhow::anyhow;
use tari_state_tree::storage::NodeKey;

use crate::codecs::{borsh::BorshCodec, DbCodec};

/// Codec for `NodeKey`
pub type NodeKeyCodec = BorshCodec<NodeKey>;

#[cfg(test)]
mod tests {
    use std::iter;

    use jmt::{storage::NibblePath, JellyfishMerkleTree};
    use rand::Rng;
    use tari_state_tree::{memory_store::MemoryTreeStore, KeyHash, TreeStoreBatchWriter, TreeStoreWriter};

    use super::*;

    fn make_nibble_path<T: AsRef<[u8]>>(nibbles: T) -> NibblePath {
        nibbles.as_ref().iter().map(|&b| b.into()).collect()
    }

    #[test]
    fn encode_decode() {
        let version = 1;
        // odd (2 bytes)
        let nibble_path = make_nibble_path([0x01, 0x02, 0x03 << 4 + 0x04]);
        let key = NodeKey::new(version, nibble_path);
        let codec = NodeKeyCodec::new();
        let encoded1 = codec.encode(&key).unwrap();
        let decoded = codec.decode(&encoded1).unwrap();
        assert_eq!(key, decoded);

        let version = 2;
        // even (2 bytes)
        let nibble_path = make_nibble_path([0x01, 0x02, 0x03, 0x04]);
        let key = NodeKey::new(version, nibble_path);

        let encoded = codec.encode(&key).unwrap();
        assert_ne!(encoded, encoded1);
        let decoded = codec.decode(&encoded).unwrap();
        assert_eq!(key, decoded);
    }

    #[test]
    fn smoke() {
        let mut store = MemoryTreeStore::new();
        let jmt = JellyfishMerkleTree::new(&store);
        let changes = iter::repeat_with(|| KeyHash(random_bytes()))
            .take(100)
            .enumerate()
            .map(|(i, hash)| (hash, Some(vec![(i % u8::MAX as usize) as u8; 10])));

        let (r1, update) = jmt.put_value_set(changes, 1).unwrap();

        let codec = NodeKeyCodec::new();
        for (key, node) in update.node_batch.nodes() {
            let encoded = codec.encode(&key).unwrap();
            let decoded = codec.decode(&encoded).unwrap();
            assert_eq!(*key, decoded);
        }
        store.batch_insert_nodes(update.node_batch).unwrap();
        store.record_stale_tree_nodes(1, update.stale_node_index_batch).unwrap();

        let jmt = JellyfishMerkleTree::new(&store);
        let changes = iter::repeat_with(|| KeyHash(random_bytes()))
            .take(100)
            .map(|hash| (hash, Some(vec![1u8; 10])))
            .collect::<Vec<_>>();
        let (r2, update) = jmt.put_value_set(changes.clone(), 2).unwrap();

        for (key, node) in update.node_batch.nodes() {
            let encoded = codec.encode(&key).unwrap();
            let decoded = codec.decode(&encoded).unwrap();
            assert_eq!(*key, decoded);
        }

        store.batch_insert_nodes(update.node_batch).unwrap();
        store.record_stale_tree_nodes(1, update.stale_node_index_batch).unwrap();

        let jmt = JellyfishMerkleTree::new(&store);
        let (r3, _) = jmt
            .put_value_set(changes.into_iter().map(|(key, _)| (key, None)), 3)
            .unwrap();
        assert_ne!(r1, r2);
        assert_ne!(r2, r3);
    }

    fn random_bytes<const L: usize>() -> [u8; L] {
        let mut bytes = [0u8; L];
        rand::thread_rng().fill(&mut bytes[..]);
        bytes
    }
}
