//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use serde::Serialize;
use tari_dan_common_types::Epoch;

use super::{
    ForeignProposalMessage,
    ForeignProposalNotificationMessage,
    ForeignProposalRequestMessage,
    MissingTransactionsResponse,
    NewViewMessage,
    ProposalMessage,
    VoteMessage,
};
use crate::messages::{MissingTransactionsRequest, SyncRequestMessage, SyncResponseMessage};

// Serialize is implemented for the message logger
#[derive(Debug, Clone, Serialize)]
pub enum HotstuffMessage {
    NewView(NewViewMessage),
    Proposal(ProposalMessage),
    ForeignProposal(ForeignProposalMessage),
    ForeignProposalNotification(ForeignProposalNotificationMessage),
    ForeignProposalRequest(ForeignProposalRequestMessage),
    Vote(VoteMessage),
    MissingTransactionsRequest(MissingTransactionsRequest),
    MissingTransactionsResponse(MissingTransactionsResponse),
    CatchUpSyncRequest(SyncRequestMessage),
    // TODO: remove unused
    SyncResponse(SyncResponseMessage),
}

impl HotstuffMessage {
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
            Self::SyncResponse(_) => "SyncResponse",
        }
    }

    pub fn epoch(&self) -> Epoch {
        match self {
            Self::NewView(msg) => msg.high_qc.epoch(),
            Self::Proposal(msg) => msg.block.epoch(),
            Self::ForeignProposal(msg) => msg.block.epoch(),
            Self::ForeignProposalNotification(msg) => msg.epoch,
            Self::ForeignProposalRequest(msg) => msg.epoch(),
            Self::Vote(msg) => msg.epoch,
            Self::MissingTransactionsRequest(msg) => msg.epoch,
            Self::MissingTransactionsResponse(msg) => msg.epoch,
            Self::CatchUpSyncRequest(msg) => msg.high_qc.epoch(),
            Self::SyncResponse(msg) => msg.epoch,
        }
    }

    pub fn proposal(&self) -> Option<&ProposalMessage> {
        match self {
            Self::Proposal(msg) => Some(msg),
            _ => None,
        }
    }
}

impl Display for HotstuffMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HotstuffMessage::NewView(msg) => {
                write!(
                    f,
                    "NewView({}, {}, high-qc: {})",
                    msg.new_height,
                    msg.high_qc.epoch(),
                    msg.high_qc.block_height()
                )
            },
            HotstuffMessage::Proposal(msg) => {
                write!(
                    f,
                    "Proposal(Epoch={},Height={},QC={})",
                    msg.block.epoch(),
                    msg.block.height(),
                    msg.block.justify().block_height()
                )
            },
            HotstuffMessage::ForeignProposal(msg) => write!(f, "ForeignProposal({})", msg),
            HotstuffMessage::ForeignProposalNotification(msg) => write!(f, "ForeignProposalNotification({})", msg),
            HotstuffMessage::ForeignProposalRequest(msg) => write!(f, "ForeignProposalRequest({})", msg),
            HotstuffMessage::Vote(msg) => write!(
                f,
                "Vote({}, {}, {}, {})",
                msg.unverified_block_height, msg.epoch, msg.block_id, msg.decision,
            ),
            HotstuffMessage::MissingTransactionsRequest(msg) => {
                write!(
                    f,
                    "RequestMissingTransactions({} transaction(s), block: {}, epoch: {})",
                    msg.transactions.len(),
                    msg.block_id,
                    msg.epoch
                )
            },
            HotstuffMessage::MissingTransactionsResponse(msg) => write!(
                f,
                "RequestedTransaction({} transaction(s), block: {}, epoch: {})",
                msg.transactions.len(),
                msg.block_id,
                msg.epoch
            ),
            HotstuffMessage::CatchUpSyncRequest(msg) => write!(f, "SyncRequest({})", msg.high_qc),
            HotstuffMessage::SyncResponse(msg) => {
                write!(f, "SyncResponse({}, {} block(s))", msg.epoch, msg.blocks.len())
            },
        }
    }
}
