//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use tari_consensus_types::Vote;
use tari_ootle_common_types::{Epoch, displayable::Displayable};

use super::{
    ForeignProposalMessage,
    ForeignProposalNotificationMessage,
    ForeignProposalRequestMessage,
    MissingTransactionsResponse,
    NewViewMessage,
    ProposalMessage,
    VoteMessage,
};
use crate::messages::{CatchUpRequestMessage, MissingTransactionsRequest};

// Serialize is implemented for the message logger
#[derive(Debug, Clone, serde::Serialize)]
pub enum HotstuffMessage {
    /// New view message: a vote to move to a new view after a leader timeout
    NewView(Box<NewViewMessage>),
    /// Proposal message: a proposed block from the leader
    Proposal(Box<ProposalMessage>),
    /// Foreign proposal message: proposal from a foreign shard that involves this shard
    ForeignProposal(ForeignProposalMessage),
    /// A broadcast notification that a foreign proposal should be requested if not already known
    ForeignProposalNotification(ForeignProposalNotificationMessage),
    /// A request for a foreign proposal (after recieving a notification)
    ForeignProposalRequest(ForeignProposalRequestMessage),
    /// Vote message: a vote for a proposed block
    Vote(VoteMessage),
    /// Request for missing transactions
    MissingTransactionsRequest(MissingTransactionsRequest),
    /// Response with missing transactions
    MissingTransactionsResponse(MissingTransactionsResponse),
    /// Request to send blocks from a given height
    CatchUpSyncRequest(CatchUpRequestMessage),
    /// A block delivered in response to a catch-up sync request. Unlike a [`HotstuffMessage::Proposal`],
    /// this is an ordered block import: it is not subject to the live-consensus view filter and is
    /// processed regardless of the current view.
    CatchUpSyncResponse(Box<ProposalMessage>),
}

impl HotstuffMessage {
    pub fn new_proposal(message: ProposalMessage) -> Self {
        Self::Proposal(Box::new(message))
    }

    pub fn new_catch_up_sync_response(message: ProposalMessage) -> Self {
        Self::CatchUpSyncResponse(Box::new(message))
    }

    pub fn new_newview(message: NewViewMessage) -> Self {
        Self::NewView(Box::new(message))
    }

    pub fn as_type_str(&self) -> &'static str {
        match self {
            Self::NewView(_) => "NewView",
            Self::Proposal(_) => "Proposal",
            Self::ForeignProposal(_) => "ForeignProposal",
            Self::ForeignProposalNotification(_) => "ForeignProposalNotification",
            Self::ForeignProposalRequest(_) => "ForeignProposalRequest",
            Self::Vote(_) => "Vote",
            Self::MissingTransactionsRequest(_) => "MissingTransactionsRequest",
            Self::MissingTransactionsResponse(_) => "MissingTransactionsResponse",
            Self::CatchUpSyncRequest(_) => "CatchUpSyncRequest",
            Self::CatchUpSyncResponse(_) => "CatchUpSyncResponse",
        }
    }

    pub fn epoch(&self) -> Epoch {
        match self {
            Self::NewView(msg) => msg.high_pc.epoch(),
            Self::Proposal(msg) => msg.block.epoch(),
            Self::ForeignProposal(msg) => msg.proposal.epoch(),
            Self::ForeignProposalNotification(msg) => msg.epoch,
            Self::ForeignProposalRequest(msg) => msg.epoch(),
            Self::Vote(msg) => msg.vote.epoch,
            Self::MissingTransactionsRequest(msg) => msg.epoch,
            Self::MissingTransactionsResponse(msg) => msg.epoch,
            Self::CatchUpSyncRequest(msg) => msg.epoch,
            Self::CatchUpSyncResponse(msg) => msg.block.epoch(),
        }
    }

    pub fn proposal(&self) -> Option<&ProposalMessage> {
        match self {
            Self::Proposal(msg) => Some(msg),
            _ => None,
        }
    }

    pub fn into_proposal(self) -> Option<ProposalMessage> {
        match self {
            Self::Proposal(msg) => Some(*msg),
            _ => None,
        }
    }
}

impl Display for HotstuffMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NewView(msg) => {
                write!(
                    f,
                    "NewView({}, {}, high-qc: {})",
                    msg.timeout,
                    msg.high_pc.epoch(),
                    msg.high_pc.height()
                )
            },
            Self::Proposal(msg) => {
                write!(
                    f,
                    "Proposal(Epoch={},Height={},QC={},TC={},#foreign={})",
                    msg.block.epoch(),
                    msg.block.height(),
                    msg.block.justify().height(),
                    msg.block.timeout_certificate().display(),
                    msg.foreign_proposals.len()
                )
            },
            Self::ForeignProposal(msg) => write!(f, "ForeignProposal({})", msg),
            Self::ForeignProposalNotification(msg) => write!(f, "ForeignProposalNotification({})", msg),
            Self::ForeignProposalRequest(msg) => write!(f, "ForeignProposalRequest({})", msg),
            Self::Vote(VoteMessage { vote }) => write!(
                f,
                "Vote({}, {}, {}, {})",
                vote.height(),
                vote.epoch,
                vote.block_id,
                vote.decision,
            ),
            Self::MissingTransactionsRequest(msg) => {
                write!(
                    f,
                    "RequestMissingTransactions({} transaction(s), block: {}, epoch: {})",
                    msg.transactions.len(),
                    msg.block_id,
                    msg.epoch
                )
            },
            Self::MissingTransactionsResponse(msg) => write!(
                f,
                "RequestedTransaction({} transaction(s), block: {}, epoch: {})",
                msg.transactions.len(),
                msg.block_id,
                msg.epoch
            ),
            Self::CatchUpSyncRequest(msg) => write!(f, "SyncRequest({}/{})", msg.epoch, msg.block_height),
            Self::CatchUpSyncResponse(msg) => {
                write!(
                    f,
                    "CatchUpSyncResponse(Epoch={},Height={},QC={},TC={},#foreign={})",
                    msg.block.epoch(),
                    msg.block.height(),
                    msg.block.justify().height(),
                    msg.block.timeout_certificate().display(),
                    msg.foreign_proposals.len()
                )
            },
        }
    }
}
