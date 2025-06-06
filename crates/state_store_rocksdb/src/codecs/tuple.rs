//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use tari_consensus_types::BlockId;
use tari_ootle_common_types::{shard::Shard, Epoch, NodeHeight};
use tari_ootle_storage::consensus_models::{ForeignProposalStatus, StateTransitionId};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
use tari_transaction::TransactionId;

use crate::{
    codecs::{Column, DbCodec, EncodeVec, EpochCodec, NumberCodec, ShardCodec},
    error::RocksDbStorageError,
};

/// A codec for encoding and decoding a tuple of two types that own a byte slice. These types must implement
/// `AsRef<[u8]>` and `TryFrom<&[u8]>`.
#[derive(Debug, Clone, Copy)]
pub struct TupleBytesCodec<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<A, B> DbCodec<(A, B)> for TupleBytesCodec<(A, B)>
where
    A: Clone + serde::Serialize + serde::de::DeserializeOwned,
    A: AsRef<[u8]> + FixedByteLength,
    for<'a> A: TryFrom<&'a [u8]>,
    for<'a> <A as TryFrom<&'a [u8]>>::Error: std::error::Error,
    B: AsRef<[u8]>,
    for<'b> B: TryFrom<&'b [u8]>,
    for<'b> <B as TryFrom<&'b [u8]>>::Error: std::error::Error,
{
    fn encode(&self, (a, b): &(A, B)) -> Result<EncodeVec, RocksDbStorageError> {
        let a_bytes = a.as_ref();
        let b_bytes = b.as_ref();
        Ok(EncodeVec::from_slices(&[a_bytes, b_bytes]))
    }

    fn decode(&self, bytes: &[u8]) -> Result<(A, B), RocksDbStorageError> {
        let (a_bytes, b_bytes) = bytes.split_at(A::BYTE_LENGTH);
        let a = A::try_from(a_bytes).map_err(|e| RocksDbStorageError::DecodeError {
            source: anyhow!("{}", e),
        })?;
        let b = B::try_from(b_bytes).map_err(|e| RocksDbStorageError::DecodeError {
            source: anyhow!("{}", e),
        })?;
        Ok((a, b))
    }
}

impl<A, B, C> DbCodec<(A, B, C)> for TupleBytesCodec<(A, B, C)>
where
    A: Clone + serde::Serialize + serde::de::DeserializeOwned,
    A: AsRef<[u8]> + FixedByteLength,
    for<'a> A: TryFrom<&'a [u8]>,
    for<'a> <A as TryFrom<&'a [u8]>>::Error: std::error::Error,
    B: AsRef<[u8]> + FixedByteLength,
    for<'b> B: TryFrom<&'b [u8]>,
    for<'b> <B as TryFrom<&'b [u8]>>::Error: std::error::Error,
    C: AsRef<[u8]>,
    for<'c> C: TryFrom<&'c [u8]>,
    for<'c> <C as TryFrom<&'c [u8]>>::Error: std::error::Error,
{
    fn encode(&self, (a, b, c): &(A, B, C)) -> Result<EncodeVec, RocksDbStorageError> {
        let a_bytes = a.as_ref();
        let b_bytes = b.as_ref();
        let c_bytes = c.as_ref();
        Ok(EncodeVec::from_slices(&[a_bytes, b_bytes, c_bytes]))
    }

    fn decode(&self, bytes: &[u8]) -> Result<(A, B, C), RocksDbStorageError> {
        let (a_bytes, rest) = bytes.split_at(A::BYTE_LENGTH);
        let a = A::try_from(a_bytes).map_err(|e| RocksDbStorageError::DecodeError {
            source: anyhow!("{}", e),
        })?;
        let (b_bytes, c_bytes) = rest.split_at(B::BYTE_LENGTH);
        let b = B::try_from(b_bytes).map_err(|e| RocksDbStorageError::DecodeError {
            source: anyhow!("{}", e),
        })?;
        let c = C::try_from(c_bytes).map_err(|e| RocksDbStorageError::DecodeError {
            source: anyhow!("{}", e),
        })?;
        Ok((a, b, c))
    }
}

impl<T> Default for TupleBytesCodec<T> {
    fn default() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<A, B, C, CA, CB, CC> DbCodec<(A, B, C)> for (CA, CB, CC)
where
    CA: DbCodec<A>,
    CB: DbCodec<B>,
    CC: DbCodec<C>,
    A: FixedByteLength,
    B: FixedByteLength,
{
    fn encode(&self, (a, b, c): &(A, B, C)) -> Result<EncodeVec, RocksDbStorageError> {
        let (ca, cb, cc) = &self;
        let a_bytes = ca.encode(a)?;
        let b_bytes = cb.encode(b)?;
        let c_bytes = cc.encode(c)?;
        Ok(EncodeVec::from_slices(&[
            a_bytes.as_ref(),
            b_bytes.as_ref(),
            c_bytes.as_ref(),
        ]))
    }

    fn decode(&self, bytes: &[u8]) -> Result<(A, B, C), RocksDbStorageError> {
        let (ca, cb, cc) = &self;
        let (a_bytes, rest) = bytes.split_at(A::BYTE_LENGTH);
        let a = ca.decode(a_bytes)?;
        let (b_bytes, c_bytes) = rest.split_at(B::BYTE_LENGTH);
        let b = cb.decode(b_bytes)?;
        let c = cc.decode(c_bytes)?;
        Ok((a, b, c))
    }
}

impl<A, B, CA, CB> DbCodec<(A, B)> for (CA, CB)
where
    CA: DbCodec<A>,
    CB: DbCodec<B>,
    A: FixedByteLength,
{
    fn encode(&self, (a, b): &(A, B)) -> Result<EncodeVec, RocksDbStorageError> {
        let a_bytes = self.0.encode(a)?;
        let b_bytes = self.1.encode(b)?;
        Ok(EncodeVec::from_slices(&[a_bytes.as_ref(), b_bytes.as_ref()]))
    }

    fn decode(&self, bytes: &[u8]) -> Result<(A, B), RocksDbStorageError> {
        let (a_bytes, b_bytes) = bytes.split_at(A::BYTE_LENGTH);
        let a = self.0.decode(a_bytes)?;
        let b = self.1.decode(b_bytes)?;
        Ok((a, b))
    }
}

/// Encodes in (A, B, C) order. (Shard, Seq, Epoch) and (Epoch, Shard, Seq) keys are supported.
#[derive(Default)]
pub struct StateTransitionIdCodec<A, B, C> {
    codec: (A, B, C),
}

impl DbCodec<StateTransitionId> for StateTransitionIdCodec<ShardCodec, NumberCodec<u64>, EpochCodec> {
    fn encode(&self, id: &StateTransitionId) -> Result<EncodeVec, RocksDbStorageError> {
        self.codec.encode(&(id.shard(), id.seq(), id.epoch()))
    }

    fn decode(&self, bytes: &[u8]) -> Result<StateTransitionId, RocksDbStorageError> {
        let (shard, seq, epoch) = self.codec.decode(bytes)?;
        Ok(StateTransitionId::new(epoch, shard, seq))
    }
}

impl DbCodec<StateTransitionId> for StateTransitionIdCodec<EpochCodec, ShardCodec, NumberCodec<u64>> {
    fn encode(&self, id: &StateTransitionId) -> Result<EncodeVec, RocksDbStorageError> {
        self.codec.encode(&(id.epoch(), id.shard(), id.seq()))
    }

    fn decode(&self, bytes: &[u8]) -> Result<StateTransitionId, RocksDbStorageError> {
        let (epoch, shard, seq) = self.codec.decode(bytes)?;
        Ok(StateTransitionId::new(epoch, shard, seq))
    }
}

pub trait FixedByteLength {
    const BYTE_LENGTH: usize;
}

impl FixedByteLength for u64 {
    const BYTE_LENGTH: usize = size_of::<u64>();
}

impl FixedByteLength for BlockId {
    const BYTE_LENGTH: usize = BlockId::byte_size();
}

impl FixedByteLength for Epoch {
    const BYTE_LENGTH: usize = size_of::<u64>();
}

impl FixedByteLength for ForeignProposalStatus {
    const BYTE_LENGTH: usize = 1;
}

impl FixedByteLength for TransactionId {
    const BYTE_LENGTH: usize = TransactionId::byte_size();
}

impl FixedByteLength for NodeHeight {
    const BYTE_LENGTH: usize = size_of::<NodeHeight>();
}

impl FixedByteLength for RistrettoPublicKeyBytes {
    const BYTE_LENGTH: usize = RistrettoPublicKeyBytes::length();
}

impl<const COL: u32> FixedByteLength for Column<COL> {
    const BYTE_LENGTH: usize = size_of::<u32>();
}

impl FixedByteLength for Shard {
    const BYTE_LENGTH: usize = size_of::<Shard>();
}
