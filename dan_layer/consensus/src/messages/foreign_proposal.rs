//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use serde::Serialize;
use tari_consensus_types::BlockId;
use tari_dan_common_types::{Epoch, ShardGroup};
use tari_dan_storage::consensus_models::{ForeignParkedProposal, ForeignProposal, ForeignProposalRecord};
use tari_transaction::TransactionId;

#[derive(Debug, Clone, Serialize)]
pub struct ForeignProposalMessage {
    pub proposal: Box<ForeignProposal>,
}

impl From<ForeignProposalMessage> for ForeignParkedProposal {
    fn from(msg: ForeignProposalMessage) -> Self {
        let (commit_proof, block_pledge) = msg.proposal.into_parts();
        ForeignParkedProposal::new(commit_proof, block_pledge)
    }
}

impl From<ForeignProposalMessage> for ForeignProposalRecord {
    fn from(msg: ForeignProposalMessage) -> Self {
        ForeignProposalRecord::new(*msg.proposal)
    }
}

impl From<ForeignProposal> for ForeignProposalMessage {
    fn from(proposal: ForeignProposal) -> Self {
        ForeignProposalMessage {
            proposal: Box::new(proposal),
        }
    }
}

impl From<ForeignParkedProposal> for ForeignProposalMessage {
    fn from(proposal: ForeignParkedProposal) -> Self {
        let proposal = proposal.into_proposal();
        ForeignProposalMessage {
            proposal: Box::new(proposal),
        }
    }
}

impl Display for ForeignProposalMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ForeignProposalMessage({})", self.proposal,)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ForeignProposalNotificationMessage {
    pub block_id: BlockId,
    pub epoch: Epoch,
}

impl Display for ForeignProposalNotificationMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ForeignProposalNotificationMessage({}, {})",
            self.epoch, self.block_id
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub enum ForeignProposalRequestMessage {
    /// Request a foreign proposal for a specific block ID in response to a ForeignProposalNotificationMessage that has
    /// not already been received.
    ByBlockId {
        block_id: BlockId,
        /// The shard group to return pledges for
        for_shard_group: ShardGroup,
        epoch: Epoch,
    },
    /// Request a foreign proposal for a specific transaction ID. This is used as a "bump" when a transaction is
    /// reached LocalPrepared but the foreign proposal has not yet been received after some time.
    ByTransactionId {
        transaction_id: TransactionId,
        for_shard_group: ShardGroup,
        epoch: Epoch,
    },
}

impl ForeignProposalRequestMessage {
    pub fn epoch(&self) -> Epoch {
        match self {
            Self::ByBlockId { epoch, .. } => *epoch,
            Self::ByTransactionId { epoch, .. } => *epoch,
        }
    }
}

impl Display for ForeignProposalRequestMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ByBlockId {
                block_id,
                for_shard_group,
                epoch,
            } => {
                write!(
                    f,
                    "ForeignProposalRequestMessage(ByBlockId, {}, {}, {})",
                    epoch, for_shard_group, block_id
                )
            },
            Self::ByTransactionId {
                transaction_id,
                for_shard_group,
                epoch,
            } => {
                write!(
                    f,
                    "ForeignProposalRequestMessage(ByTransactionId, {}, {}, {})",
                    epoch, for_shard_group, transaction_id
                )
            },
        }
    }
}
