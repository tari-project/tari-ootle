//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use serde::Serialize;
use tari_dan_common_types::{Epoch, ShardGroup};
use tari_dan_storage::consensus_models::{
    Block,
    BlockId,
    BlockPledge,
    ForeignParkedProposal,
    ForeignProposal,
    QuorumCertificate,
};
use tari_transaction::TransactionId;

#[derive(Debug, Clone, Serialize)]
pub struct ForeignProposalMessage {
    pub block: Block,
    pub justify_qc: QuorumCertificate,
    pub block_pledge: BlockPledge,
}

impl From<ForeignProposalMessage> for ForeignParkedProposal {
    fn from(msg: ForeignProposalMessage) -> Self {
        ForeignParkedProposal::new(msg.into())
    }
}

impl From<ForeignProposalMessage> for ForeignProposal {
    fn from(msg: ForeignProposalMessage) -> Self {
        ForeignProposal::new(msg.block, msg.block_pledge, msg.justify_qc)
    }
}

impl From<ForeignProposal> for ForeignProposalMessage {
    fn from(proposal: ForeignProposal) -> Self {
        ForeignProposalMessage {
            block: proposal.block,
            justify_qc: proposal.justify_qc,
            block_pledge: proposal.block_pledge,
        }
    }
}

impl From<ForeignParkedProposal> for ForeignProposalMessage {
    fn from(proposal: ForeignParkedProposal) -> Self {
        let proposal = proposal.into_proposal();
        ForeignProposalMessage {
            block: proposal.block,
            justify_qc: proposal.justify_qc,
            block_pledge: proposal.block_pledge,
        }
    }
}

impl Display for ForeignProposalMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ForeignProposalMessage({}, {}, {} pledged substate(s))",
            self.block,
            self.justify_qc,
            self.block_pledge.num_substates_pledged()
        )
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
