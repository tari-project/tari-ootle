//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::marker::PhantomData;

use serde::{Deserialize, Serialize};
use tari_dan_common_types::Epoch;
use tari_dan_storage::consensus_models::{
    BlockId,
    HighQc,
    LastExecuted,
    LastProposed,
    LastSentVote,
    LastVoted,
    LeafBlock,
    LockedBlock,
};

use crate::{
    codecs::{BookkeepingKeyCodec, DefaultCodec, DefaultCodecRef, NumberCodec},
    error::RocksDbStorageError,
    traits::{Cf, QueryCf},
};

pub enum BookkeepingKey {
    LastVoted,
    LastExecuted,
    LastProposed,
    LastSentVote,
    CommitBlock,
    // TODO: remove epoch from these
    LeafBlock(Epoch),
    LockedBlock(Epoch),
    HighQc(Epoch),
}

impl BookkeepingKey {
    pub(crate) fn epoch(&self) -> Option<Epoch> {
        match self {
            Self::LeafBlock(epoch) => Some(*epoch),
            Self::LockedBlock(epoch) => Some(*epoch),
            Self::HighQc(epoch) => Some(*epoch),
            _ => None,
        }
    }

    pub(crate) fn as_byte(&self) -> u8 {
        match self {
            Self::LastVoted => 0,
            Self::LastExecuted => 1,
            Self::LastProposed => 2,
            Self::LastSentVote => 3,
            Self::CommitBlock => 4,
            Self::LockedBlock(_) => 5,
            Self::LeafBlock(_) => 6,
            Self::HighQc(_) => 7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BookkeepingValue {
    LastVoted(LastVoted),
    LastExecuted(LastExecuted),
    LastProposed(LastProposed),
    LastSentVote(LastSentVote),
    CommitBlock { block_id: BlockId, parent_id: BlockId },
    LeafBlock(LeafBlock),
    LockedBlock(LockedBlock),
    HighQc(HighQc),
}

impl BookkeepingValue {
    // (child, parent)
    pub fn into_commit_block(self) -> Option<(BlockId, BlockId)> {
        match self {
            BookkeepingValue::CommitBlock { block_id, parent_id } => Some((block_id, parent_id)),
            _ => None,
        }
    }

    pub fn into_leaf_block(self) -> Option<LeafBlock> {
        match self {
            BookkeepingValue::LeafBlock(v) => Some(v),
            _ => None,
        }
    }
}

impl TryFrom<BookkeepingValue> for LastVoted {
    type Error = RocksDbStorageError;

    fn try_from(value: BookkeepingValue) -> Result<Self, Self::Error> {
        match value {
            BookkeepingValue::LastVoted(v) => Ok(v),
            _ => Err(RocksDbStorageError::MalformedData {
                operation: "load bookkeeping value",
                details: format!("Expected LastVoted, but got {:?}", value),
            }),
        }
    }
}

impl TryFrom<BookkeepingValue> for LastExecuted {
    type Error = RocksDbStorageError;

    fn try_from(value: BookkeepingValue) -> Result<Self, Self::Error> {
        match value {
            BookkeepingValue::LastExecuted(v) => Ok(v),
            _ => Err(RocksDbStorageError::MalformedData {
                operation: "load bookkeeping value",
                details: format!("Expected LastExecuted, but got {:?}", value),
            }),
        }
    }
}

impl TryFrom<BookkeepingValue> for LastProposed {
    type Error = RocksDbStorageError;

    fn try_from(value: BookkeepingValue) -> Result<Self, Self::Error> {
        match value {
            BookkeepingValue::LastProposed(v) => Ok(v),
            _ => Err(RocksDbStorageError::MalformedData {
                operation: "load bookkeeping value",
                details: format!("Expected LastProposed, but got {:?}", value),
            }),
        }
    }
}

impl TryFrom<BookkeepingValue> for LastSentVote {
    type Error = RocksDbStorageError;

    fn try_from(value: BookkeepingValue) -> Result<Self, Self::Error> {
        match value {
            BookkeepingValue::LastSentVote(v) => Ok(v),
            _ => Err(RocksDbStorageError::MalformedData {
                operation: "load bookkeeping value",
                details: format!("Expected LastSentVote, but got {:?}", value),
            }),
        }
    }
}

impl TryFrom<BookkeepingValue> for LeafBlock {
    type Error = RocksDbStorageError;

    fn try_from(value: BookkeepingValue) -> Result<Self, Self::Error> {
        match value {
            BookkeepingValue::LeafBlock(v) => Ok(v),
            _ => Err(RocksDbStorageError::MalformedData {
                operation: "load bookkeeping value",
                details: format!("Expected LeafBlock, but got {:?}", value),
            }),
        }
    }
}

impl TryFrom<BookkeepingValue> for LockedBlock {
    type Error = RocksDbStorageError;

    fn try_from(value: BookkeepingValue) -> Result<Self, Self::Error> {
        match value {
            BookkeepingValue::LockedBlock(v) => Ok(v),
            _ => Err(RocksDbStorageError::MalformedData {
                operation: "load bookkeeping value",
                details: format!("Expected LockedBlock, but got {:?}", value),
            }),
        }
    }
}

impl TryFrom<BookkeepingValue> for HighQc {
    type Error = RocksDbStorageError;

    fn try_from(value: BookkeepingValue) -> Result<Self, Self::Error> {
        match value {
            BookkeepingValue::HighQc(v) => Ok(v),
            _ => Err(RocksDbStorageError::MalformedData {
                operation: "load bookkeeping value",
                details: format!("Expected HighQc, but got {:?}", value),
            }),
        }
    }
}

#[derive(Debug, Serialize)]
pub enum BookkeepingValueRef<'a> {
    LastVoted(&'a LastVoted),
    LastExecuted(&'a LastExecuted),
    LastProposed(&'a LastProposed),
    LastSentVote(&'a LastSentVote),
    CommitBlock {
        block_id: &'a BlockId,
        parent_id: &'a BlockId,
    },
    LeafBlock(&'a LeafBlock),
    LockedBlock(&'a LockedBlock),
    HighQc(&'a HighQc),
}

impl<'a> From<&'a BookkeepingValue> for BookkeepingValueRef<'a> {
    fn from(value: &'a BookkeepingValue) -> Self {
        match value {
            BookkeepingValue::LastVoted(v) => BookkeepingValueRef::LastVoted(v),
            BookkeepingValue::LastExecuted(v) => BookkeepingValueRef::LastExecuted(v),
            BookkeepingValue::LastProposed(v) => BookkeepingValueRef::LastProposed(v),
            BookkeepingValue::LastSentVote(v) => BookkeepingValueRef::LastSentVote(v),
            BookkeepingValue::CommitBlock { block_id, parent_id } => {
                BookkeepingValueRef::CommitBlock { block_id, parent_id }
            },
            BookkeepingValue::LeafBlock(v) => BookkeepingValueRef::LeafBlock(v),
            BookkeepingValue::LockedBlock(v) => BookkeepingValueRef::LockedBlock(v),
            BookkeepingValue::HighQc(v) => BookkeepingValueRef::HighQc(v),
        }
    }
}

impl<'a> From<&'a LastVoted> for BookkeepingValueRef<'a> {
    fn from(value: &'a LastVoted) -> Self {
        BookkeepingValueRef::LastVoted(value)
    }
}

impl<'a> From<&'a LastExecuted> for BookkeepingValueRef<'a> {
    fn from(value: &'a LastExecuted) -> Self {
        BookkeepingValueRef::LastExecuted(value)
    }
}

impl<'a> From<&'a LastProposed> for BookkeepingValueRef<'a> {
    fn from(value: &'a LastProposed) -> Self {
        BookkeepingValueRef::LastProposed(value)
    }
}

impl<'a> From<&'a LastSentVote> for BookkeepingValueRef<'a> {
    fn from(value: &'a LastSentVote) -> Self {
        BookkeepingValueRef::LastSentVote(value)
    }
}

impl<'a> From<&'a LeafBlock> for BookkeepingValueRef<'a> {
    fn from(value: &'a LeafBlock) -> Self {
        BookkeepingValueRef::LeafBlock(value)
    }
}

impl<'a> From<&'a LockedBlock> for BookkeepingValueRef<'a> {
    fn from(value: &'a LockedBlock) -> Self {
        BookkeepingValueRef::LockedBlock(value)
    }
}

impl<'a> From<&'a HighQc> for BookkeepingValueRef<'a> {
    fn from(value: &'a HighQc) -> Self {
        BookkeepingValueRef::HighQc(value)
    }
}

pub struct BookkeepingModel;

impl Cf for BookkeepingModel {
    type Key = BookkeepingKey;
    type KeyCodec = BookkeepingKeyCodec;
    type Value = BookkeepingValue;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        "bookkeeping"
    }
}

#[derive(Debug, Default)]
pub struct BookkeepingModelRef<'a>(PhantomData<&'a ()>);

impl<'a> Cf for BookkeepingModelRef<'a> {
    type Key = BookkeepingKey;
    type KeyCodec = BookkeepingKeyCodec;
    type Value = BookkeepingValueRef<'a>;
    type ValueCodec = DefaultCodecRef<Self::Value>;

    fn name() -> &'static str {
        BookkeepingModel::name()
    }
}

pub struct ByKeyByteQuery;

impl QueryCf for ByKeyByteQuery {
    type Cf = BookkeepingModel;
    type Key = u8;
    type KeyCodec = NumberCodec<Self::Key>;
}
