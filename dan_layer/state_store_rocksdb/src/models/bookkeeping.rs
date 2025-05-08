//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_dan_common_types::NodeHeight;
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
    codecs::{ByteColumn, ColumnCodec, DefaultCodec},
    traits::Cf,
};

pub const CF_NAME: &str = "bookkeeping";

enum BookKeepingKey {
    LastVoted,
    LastExecuted,
    LastProposed,
    LastSentVote,
    CommitBlock,
    LeafBlock,
    LockedBlock,
    HighQc,
}

impl BookKeepingKey {
    const fn as_byte(&self) -> u8 {
        match self {
            Self::LastVoted => 0,
            Self::LastExecuted => 1,
            Self::LastProposed => 2,
            Self::LastSentVote => 3,
            Self::CommitBlock => 4,
            Self::LockedBlock => 5,
            Self::LeafBlock => 6,
            Self::HighQc => 7,
        }
    }
}

pub struct LastVotedModel;

impl Cf for LastVotedModel {
    type Key = ByteColumn<{ BookKeepingKey::LastVoted.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = LastVoted;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

pub struct LastExecutedModel;

impl Cf for LastExecutedModel {
    type Key = ByteColumn<{ BookKeepingKey::LastExecuted.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = LastExecuted;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

pub struct LastProposedModel;

impl Cf for LastProposedModel {
    type Key = ByteColumn<{ BookKeepingKey::LastProposed.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = LastProposed;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

pub struct LastSentVoteModel;

impl Cf for LastSentVoteModel {
    type Key = ByteColumn<{ BookKeepingKey::LastSentVote.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = LastSentVote;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitBlock {
    pub height: NodeHeight,
    pub block_id: BlockId,
    pub parent_id: BlockId,
}

pub struct CommitBlockModel;

impl Cf for CommitBlockModel {
    type Key = ByteColumn<{ BookKeepingKey::CommitBlock.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = CommitBlock;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

pub struct LeafBlockModel;

impl Cf for LeafBlockModel {
    type Key = ByteColumn<{ BookKeepingKey::LeafBlock.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = LeafBlock;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

pub struct LockedBlockModel;

impl Cf for LockedBlockModel {
    type Key = ByteColumn<{ BookKeepingKey::LockedBlock.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = LockedBlock;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

pub struct HighQcModel;

impl Cf for HighQcModel {
    type Key = ByteColumn<{ BookKeepingKey::HighQc.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = HighQc;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}
