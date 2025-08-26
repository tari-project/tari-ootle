//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod bincode;
mod block_diff;
mod bytes;
mod column;
mod misc;
mod public_key;
mod shard_group;
mod small_bytes;
mod state_tree;
mod substate_id;
mod substate_lock;
mod tuple;
mod versioned;

use std::io;

pub use bincode::*;
pub use block_diff::*;
pub use bytes::*;
pub use column::*;
pub use misc::*;
pub use public_key::*;
pub use shard_group::ShardGroupCodec;
pub use state_tree::*;
pub use substate_id::*;
pub use substate_lock::*;
pub use versioned::*;

use crate::error::RocksDbStorageError;

/// When the key is smaller than 100 bytes, it will be stack-allocated. Otherwise, it will be heap-allocated.
pub type EncodeVec = small_bytes::SmallBytes<100>;

pub trait DbCodec<T> {
    fn encode(&self, value: &T) -> Result<EncodeVec, RocksDbStorageError>;
    fn decode(&self, bytes: &[u8]) -> Result<T, RocksDbStorageError> {
        let reader = &mut &bytes[..];
        self.decode_reader(reader)
    }

    fn decode_reader<R: io::Read>(&self, reader: &mut R) -> Result<T, RocksDbStorageError>;
}

pub type DefaultCodec<T> = Bincode<T>;
pub type DefaultVersionedCodec<T> = VersionedCodec<DefaultCodec<T>, T>;
pub type DefaultCodecRef<T> = BincodeRef<T>;
