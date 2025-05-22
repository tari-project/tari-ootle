//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_consensus_types::{
    BlockId,
    HighPc,
    HighTc,
    HighestSeenBlock,
    LastExecuted,
    LastProposed,
    LastSentVote,
    LastVoted,
    LeafBlock,
    LockedBlock,
};
use tari_dan_common_types::NodeHeight;
use tari_dan_storage::consensus_models::EpochStateRoot;

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
    /// The last high proposal certificate
    HighQc,
    /// The last high timeout certificate
    HighTc,
    /// The state root of the previous epoch. This is set based on either calculated root of the current shard group
    /// after a successful sync, or based on the last checkpoint
    PreviousEpochStateRoot,
    /// The highest block seen by the node
    HighestSeenBlock,
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
            Self::HighTc => 9,
            Self::PreviousEpochStateRoot => 10,
            Self::HighestSeenBlock => 11,
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

pub struct LastVotedCf;

impl Cf for LastVotedCf {
    type Key = ByteColumn<{ BookKeepingKey::LastVoted.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = LastVoted;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

pub struct LastExecutedCf;

impl Cf for LastExecutedCf {
    type Key = ByteColumn<{ BookKeepingKey::LastExecuted.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = LastExecuted;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

pub struct LastProposedCf;

impl Cf for LastProposedCf {
    type Key = ByteColumn<{ BookKeepingKey::LastProposed.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = LastProposed;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

pub struct LastSentVoteCf;

impl Cf for LastSentVoteCf {
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

pub struct CommitBlockCf;

impl Cf for CommitBlockCf {
    type Key = ByteColumn<{ BookKeepingKey::CommitBlock.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = CommitBlock;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

pub struct LeafBlockCf;

impl Cf for LeafBlockCf {
    type Key = ByteColumn<{ BookKeepingKey::LeafBlock.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = LeafBlock;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

pub struct LockedBlockCf;

impl Cf for LockedBlockCf {
    type Key = ByteColumn<{ BookKeepingKey::LockedBlock.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = LockedBlock;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

pub struct HighPcCf;

impl Cf for HighPcCf {
    type Key = ByteColumn<{ BookKeepingKey::HighQc.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = HighPc;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

pub struct HighTcCf;

impl Cf for HighTcCf {
    type Key = ByteColumn<{ BookKeepingKey::HighTc.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = HighTc;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

pub struct PreviousEpochStateRootCf;

impl Cf for PreviousEpochStateRootCf {
    type Key = ByteColumn<{ BookKeepingKey::PreviousEpochStateRoot.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = EpochStateRoot;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}

pub struct HighestSeenBlockCf;

impl Cf for HighestSeenBlockCf {
    type Key = ByteColumn<{ BookKeepingKey::HighestSeenBlock.as_byte() }>;
    type KeyCodec = ColumnCodec;
    type Value = HighestSeenBlock;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        CF_NAME
    }
}
