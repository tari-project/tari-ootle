//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::{
    proto,
    proto::{
        consensus::{Block, QuorumCertificate},
        rpc::{sync_blocks_response::SyncData, QuorumCertificates, SubstateCreatedProof, SubstateUpdate},
        transaction::Transaction,
    },
};

impl proto::rpc::SyncBlocksResponse {
    pub fn into_block(self) -> Option<Block> {
        match self.sync_data {
            Some(SyncData::Block(block)) => Some(block),
            _ => None,
        }
    }

    pub fn into_quorum_certificates(self) -> Option<Vec<QuorumCertificate>> {
        match self.sync_data {
            Some(SyncData::QuorumCertificates(QuorumCertificates { quorum_certificates })) => Some(quorum_certificates),
            _ => None,
        }
    }

    pub fn substate_count(&self) -> Option<u32> {
        match self.sync_data {
            Some(SyncData::SubstateCount(count)) => Some(count),
            _ => None,
        }
    }

    pub fn transaction_count(&self) -> Option<u32> {
        match self.sync_data {
            Some(SyncData::TransactionCount(count)) => Some(count),
            _ => None,
        }
    }

    pub fn into_substate_update(self) -> Option<SubstateUpdate> {
        match self.sync_data {
            Some(SyncData::SubstateUpdate(update)) => Some(update),
            _ => None,
        }
    }

    pub fn into_transaction(self) -> Option<Transaction> {
        match self.sync_data {
            Some(SyncData::Transaction(transaction)) => Some(transaction),
            _ => None,
        }
    }

    pub fn transaction_receipt_count(&self) -> Option<u32> {
        match self.sync_data {
            Some(SyncData::TransactionReceiptCount(count)) => Some(count),
            _ => None,
        }
    }

    pub fn into_transaction_receipt(self) -> Option<SubstateCreatedProof> {
        match self.sync_data {
            Some(SyncData::TransactionReceipt(receipt)) => Some(receipt),
            _ => None,
        }
    }
}
