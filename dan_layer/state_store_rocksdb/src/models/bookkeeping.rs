//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_dan_common_types::NodeHeight;
use tari_dan_storage::consensus_models::{
    BlockId,
    EpochStateRoot,
    HighQc,
    LastExecuted,
    LastProposed,
    LastSentVote,
    LastVoted,
    LeafBlock,
    LockedBlock,
};

use crate::{
    codecs::{ByteColumn, ColumnCodec, DefaultCodec, NumberCodec},
    traits::Cf,
};

pub const CF_NAME: &str = "bookkeeping";

enum BookKeepingKey {
    /// The migration version of the database
    DatabaseMigrationVersion,
    /// The last voted block
    LastVoted,
    /// The last executed block
    LastExecuted,
    /// The last proposed block
    LastProposed,
    /// The last sent vote
    LastSentVote,
    /// The last committed block
    CommitBlock,
    /// The last leaf block
    LeafBlock,
    /// The last locked block
    LockedBlock,
    /// The last high quorum certificate
    HighQc,
    /// The state root of the previous epoch. This is set based on either calculated root of the current shard group
    /// after a successful sync, or based on the last checkpoint
    PreviousEpochStateRoot,
}

impl BookKeepingKey {
    const fn as_byte(&self) -> u8 {
        match self {
            Self::DatabaseMigrationVersion => 0,
            Self::LastVoted => 1,
            Self::LastExecuted => 2,
            Self::LastProposed => 3,
            Self::LastSentVote => 4,
            Self::CommitBlock => 5,
            Self::LockedBlock => 6,
            Self::LeafBlock => 7,
            Self::HighQc => 8,
            Self::PreviousEpochStateRoot => 9,
        }
    }
}

pub struct DatabaseMigrationVersion;

impl Cf for DatabaseMigrationVersion {
    type Key = ByteColumn<{ BookKeepingKey::DatabaseMigrationVersion.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = u64;
    type ValueCodec = NumberCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
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

pub struct PreviousEpochStateRootModel;

impl Cf for PreviousEpochStateRootModel {
    type Key = ByteColumn<{ BookKeepingKey::PreviousEpochStateRoot.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = EpochStateRoot;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}
