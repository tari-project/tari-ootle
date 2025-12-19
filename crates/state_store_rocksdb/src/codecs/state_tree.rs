//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{io, io::Read};

use anyhow::anyhow;
use tari_state_tree::{NibblePath, NodeKey, Version};

use crate::{
    codecs::DbCodec,
    error::RocksDbStorageError,
    utils::{read_n_bytes, read_to_fixed},
};

/// Codec for `NodeKey`
#[derive(Default)]
pub struct NodeKeyCodec;

impl NodeKeyCodec {
    fn encode_node_key_into<W: io::Write>(&self, key: &NodeKey, writer: &mut W) -> Result<(), RocksDbStorageError> {
        let version = key.version();
        let num_nibbles =
            u64::try_from(key.nibble_path().num_nibbles()).map_err(|_| RocksDbStorageError::EncodeError {
                source: anyhow!("Number of nibbles exceeds u64"),
            })?;
        let nibble_path = key.nibble_path();
        writer
            .write_all(&version.to_be_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("NodeKeyCodec: Failed to write version: {}", e),
            })?;
        writer
            .write_all(&num_nibbles.to_be_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("NodeKeyCodec: Failed to write num_nibbles: {}", e),
            })?;
        writer
            .write_all(nibble_path.bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("NodeKeyCodec: Failed to write nibble_path bytes: {}", e),
            })?;
        Ok(())
    }

    fn get_node_key_encoded_len(&self, key: &NodeKey) -> Result<usize, RocksDbStorageError> {
        let len = 8 + // version
            8 + // num_nibbles
            key.nibble_path().bytes().len(); // nibble_path bytes
        Ok(len)
    }
}

impl DbCodec<NodeKey> for NodeKeyCodec {
    fn encode_len(&self, value: &NodeKey) -> Result<usize, RocksDbStorageError> {
        self.get_node_key_encoded_len(value)
    }

    fn encode_into<W: io::Write>(&self, value: &NodeKey, writer: &mut W) -> Result<(), RocksDbStorageError> {
        self.encode_node_key_into(value, writer)
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<NodeKey, RocksDbStorageError> {
        let buf = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("Invalid version bytes"),
        })?;
        let version = Version::from_be_bytes(buf);
        let num_nibbles = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("Invalid num_nibbles bytes"),
        })?;
        let num_nibbles =
            usize::try_from(u64::from_be_bytes(num_nibbles)).map_err(|_| RocksDbStorageError::DecodeError {
                source: anyhow!("Number of nibbles exceeds usize"),
            })?;
        let is_even = num_nibbles % 2 == 0;
        let nibble_path = if is_even {
            let num_bytes_even = num_nibbles / 2;
            let nibble_path_bytes =
                read_n_bytes(reader, num_bytes_even).ok_or_else(|| RocksDbStorageError::DecodeError {
                    source: anyhow!("Invalid nibble path bytes. Could not read {} bytes", num_bytes_even),
                })?;
            NibblePath::new_even(nibble_path_bytes)
        } else {
            let num_bytes_odd = num_nibbles.div_ceil(2);
            let nibble_path_bytes =
                read_n_bytes(reader, num_bytes_odd).ok_or_else(|| RocksDbStorageError::DecodeError {
                    source: anyhow!("Invalid nibble path bytes. Could not read {} bytes", num_bytes_odd),
                })?;
            NibblePath::new_odd(nibble_path_bytes)
        };
        Ok(NodeKey::new(version, nibble_path))
    }
}

impl<'a> DbCodec<&'a NodeKey> for NodeKeyCodec {
    fn encode_len(&self, value: &&'a NodeKey) -> Result<usize, RocksDbStorageError> {
        self.get_node_key_encoded_len(value)
    }

    fn encode_into<W: io::Write>(&self, value: &&'a NodeKey, writer: &mut W) -> Result<(), RocksDbStorageError> {
        self.encode_node_key_into(value, writer)
    }

    fn decode_reader<R: Read>(&self, _reader: &mut R) -> Result<&'a NodeKey, RocksDbStorageError> {
        unreachable!("decode should not be called on NodeKeyCodec with a reference")
    }
}

#[cfg(test)]
mod tests {
    use std::iter;

    use rand::Rng;
    use tari_state_tree::{
        memory_store::MemoryTreeStore,
        JellyfishMerkleTree,
        LeafKey,
        StaleTreeNode,
        TreeHash,
        TreeStoreWriter,
    };

    use super::*;

    #[test]
    fn encode_decode() {
        let version = 1;
        let nibble_path = NibblePath::new_odd(vec![0x01, 0x02, 0x03, 0x04 << 4]);
        let key = NodeKey::new(version, nibble_path);
        let codec = NodeKeyCodec;
        let encoded1 = codec.encode(&key).unwrap();
        let decoded = codec.decode(&encoded1).unwrap();
        assert_eq!(key, decoded);

        let version = 2;
        let nibble_path = NibblePath::new_even(vec![0x01, 0x02, 0x03, 0x04]);
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
        let changes = iter::repeat_with(|| TreeHash::new(random_bytes()))
            .take(100)
            .map(|hash| (LeafKey::new(hash), Some((hash, ()))));

        let (_, update) = jmt.batch_put_value_set(changes, None, None, 1).unwrap();

        let codec = NodeKeyCodec;
        for (key, node) in update.node_batch {
            let encoded = codec.encode(&key).unwrap();
            let decoded = codec.decode(&encoded).unwrap();
            assert_eq!(key, decoded);
            store.insert_node(key, node).unwrap()
        }
        for stale_tree_node in update.stale_node_index_batch {
            store
                .record_stale_tree_node(StaleTreeNode::Node(stale_tree_node.node_key))
                .unwrap();
        }

        let jmt = JellyfishMerkleTree::new(&store);
        let changes = iter::repeat_with(|| TreeHash::new(random_bytes()))
            .take(100)
            .map(|hash| (LeafKey::new(hash), Some((hash, ()))))
            .collect::<Vec<_>>();
        let (_, update) = jmt
            .batch_put_value_set(changes.iter().copied(), None, Some(1), 2)
            .unwrap();

        for (key, node) in update.node_batch {
            let encoded = codec.encode(&key).unwrap();
            let decoded = codec.decode(&encoded).unwrap();
            assert_eq!(key, decoded);
            store.insert_node(key, node).unwrap()
        }
        for stale_tree_node in update.stale_node_index_batch {
            store
                .record_stale_tree_node(StaleTreeNode::Node(stale_tree_node.node_key))
                .unwrap();
        }
        let jmt = JellyfishMerkleTree::new(&store);
        let _unused = jmt
            .batch_put_value_set(changes.into_iter().map(|(key, _)| (key, None)), None, Some(2), 3)
            .unwrap();
    }

    fn random_bytes<const L: usize>() -> [u8; L] {
        let mut bytes = [0u8; L];
        rand::thread_rng().fill(&mut bytes[..]);
        bytes
    }
}
