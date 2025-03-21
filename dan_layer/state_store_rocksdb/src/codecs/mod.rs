//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub use bincode::*;
pub use block_diff::*;
pub use bytes::*;
pub use column::*;
pub use misc::*;
pub use public_key::*;
pub use state_tree::*;
pub use substate_id::*;
pub use substate_lock::*;
pub use substate_transaction_index::*;
pub use timestamp::*;
pub use tuple::*;

use crate::error::RocksDbStorageError;

mod bincode;
mod block_diff;
mod bytes;
mod column;
mod misc;
mod public_key;
mod small_bytes;
mod state_tree;
mod substate_id;
mod substate_lock;
mod substate_transaction_index;
mod timestamp;
mod tuple;

/// Prevent heap allocations when the key is very small
pub type EncodeVec = small_bytes::SmallBytes<64>;

pub trait DbCodec<T> {
    fn encode(&self, value: &T) -> Result<EncodeVec, RocksDbStorageError>;

    fn decode(&self, bytes: &[u8]) -> Result<T, RocksDbStorageError>;
}

pub type DefaultCodec<T> = Bincode<T>;
pub type DefaultCodecRef<T> = BincodeRef<T>;
