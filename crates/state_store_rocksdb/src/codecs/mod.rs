//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod bincode;
mod block_diff;
mod byte_counter;
mod bytes;
mod column;
mod minicbor;
mod misc;
#[macro_use]
mod prefixed;
mod public_key;
mod serde_bridge_codec;
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
pub use minicbor::*;
pub use misc::*;
pub use prefixed::*;
pub use public_key::*;
pub use serde_bridge_codec::*;
pub use shard_group::ShardGroupCodec;
pub use state_tree::*;
pub use substate_id::*;
pub use substate_lock::*;
pub use versioned::*;

use crate::error::RocksDbStorageError;

/// When the key is smaller than 100 bytes, it will be stack-allocated. Otherwise, it will be heap-allocated.
pub type EncodeVec = small_bytes::SmallBytes<100>;

pub trait DbCodec<T>: DbEncoder<T> + DbDecoder<T> {}

impl<T, C> DbCodec<T> for C where C: DbEncoder<T> + DbDecoder<T> {}

pub trait DbEncoder<T> {
    fn encode_len(&self, value: &T) -> Result<usize, RocksDbStorageError>;
    fn encode_into<W: io::Write>(&self, value: &T, writer: &mut W) -> Result<(), RocksDbStorageError>;
    fn encode(&self, value: &T) -> Result<EncodeVec, RocksDbStorageError> {
        let len = self.encode_len(value)?;
        if len <= EncodeVec::FIXED_SIZE {
            let mut buf = EncodeVec::make_stack_buf();
            let mut buf_mut = buf.as_mut_slice();
            self.encode_into(value, &mut buf_mut)?;
            return Ok(EncodeVec::from_buf_and_len(buf, len));
        }

        let mut buf = Vec::with_capacity(len);
        self.encode_into(value, &mut buf)?;
        Ok(EncodeVec::from_vec(buf))
    }
}

pub trait DbDecoder<T> {
    fn decode(&self, bytes: &[u8]) -> Result<T, RocksDbStorageError> {
        let reader = &mut &bytes[..];
        self.decode_reader(reader)
    }

    fn decode_reader<R: io::Read>(&self, reader: &mut R) -> Result<T, RocksDbStorageError>;
}

pub type DefaultCodec<T> = Minicbor<T>;
pub type DefaultVersionedCodec<T> = VersionedCodec<DefaultCodec<T>, T>;
