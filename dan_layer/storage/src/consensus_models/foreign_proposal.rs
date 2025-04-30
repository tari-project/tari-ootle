//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt::{self, Display, Formatter},
    hash::Hash,
    ops::Deref,
    str::FromStr,
};

use borsh::BorshSerialize;
use serde::{Deserialize, Serialize};
use tari_crypto::tari_utilities::ByteArray;
use tari_dan_common_types::{committee::CommitteeInfo, Epoch, NodeHeight, ShardGroup};
use tari_sidechain::{CommitProofElement, QuorumCertificate};
use tari_template_lib::prelude::RistrettoPublicKeyBytes;
use tari_transaction::TransactionId;

use super::{BlockId, BlockPledge, Command, CommandOrHash, CommandsCommitProof, LeafBlock};
use crate::{StateStoreReadTransaction, StateStoreWriteTransaction, StorageError};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ForeignProposalRecord {
    block_id: BlockId,
    proposal: ForeignProposal,
    proposed_in_block: Option<BlockId>,
    status: ForeignProposalStatus,
}

impl ForeignProposalRecord {
    pub fn new(proposal: ForeignProposal) -> Self {
        let block_id = proposal.calculate_block_id();
        Self {
            block_id,
            proposal,
            proposed_in_block: None,
            status: ForeignProposalStatus::New,
        }
    }

    pub fn load(
        block_id: BlockId,
        proposal: ForeignProposal,
        proposed_in_block: Option<BlockId>,
        status: ForeignProposalStatus,
    ) -> Self {
        Self {
            block_id,
            proposal,
            proposed_in_block,
            status,
        }
    }

    /// Returns the atom for this proposal.
    pub fn to_atom(&self) -> ForeignProposalAtom {
        ForeignProposalAtom {
            shard_group: self.proposal.shard_group_unchecked(),
            block_id: *self.block_id(),
        }
    }

    pub fn block_id(&self) -> &BlockId {
        &self.block_id
    }

    pub fn as_leaf_block(&self) -> LeafBlock {
        LeafBlock {
            block_id: self.block_id,
            height: self.height(),
            epoch: self.epoch(),
        }
    }

    pub fn proposal(&self) -> &ForeignProposal {
        &self.proposal
    }

    pub fn into_proposal(self) -> ForeignProposal {
        self.proposal
    }

    pub fn commands(&self) -> &[CommandOrHash] {
        self.proposal.commit_proof().commands()
    }

    pub fn full_commands_iter(&self) -> impl Iterator<Item = &Command> + '_ {
        self.commands().iter().filter_map(|cmd| cmd.command())
    }

    pub fn shard_group_checked(&self) -> Option<ShardGroup> {
        self.proposal.shard_group_checked()
    }

    /// Returns the shard group for the proposal proof.
    /// The shard group bounds are not checked.
    pub fn shard_group_unchecked(&self) -> ShardGroup {
        self.proposal.shard_group_unchecked()
    }

    pub fn height(&self) -> NodeHeight {
        self.proposal.height()
    }

    pub fn epoch(&self) -> Epoch {
        self.proposal.epoch()
    }

    pub fn block_pledge(&self) -> &BlockPledge {
        self.proposal.block_pledge()
    }

    pub fn status(&self) -> ForeignProposalStatus {
        self.status
    }

    pub fn proposed_in_block(&self) -> Option<&BlockId> {
        self.proposed_in_block.as_ref()
    }

    /// Resets the proposal status to `New` and clears the `proposed_in_block`.
    pub fn reset_proposed(&mut self) -> &mut Self {
        self.proposed_in_block = None;
        self.status = ForeignProposalStatus::New;
        self
    }

    pub fn set_proposal_status(&mut self, status: ForeignProposalStatus) -> &mut Self {
        self.status = status;
        self
    }

    pub fn set_proposed_in_block(&mut self, block_id: BlockId) -> &mut Self {
        self.proposed_in_block = Some(block_id);
        self
    }
}

impl ForeignProposalRecord {
    pub fn save<TTx>(&self, tx: &mut TTx) -> Result<(), StorageError>
    where
        TTx: StateStoreWriteTransaction + Deref,
        TTx::Target: StateStoreReadTransaction,
    {
        tx.foreign_proposals_save(self)
    }

    pub fn set_status<TTx: StateStoreWriteTransaction>(
        &mut self,
        tx: &mut TTx,
        status: ForeignProposalStatus,
        set_proposed_in_block: Option<&BlockId>,
    ) -> Result<(), StorageError> {
        self.status = status;
        if let Some(proposed_in_block) = set_proposed_in_block {
            self.proposed_in_block = Some(*proposed_in_block);
        }
        tx.foreign_proposals_set_status(self.block_id(), status, set_proposed_in_block)
    }

    pub fn set_status_by_id<TTx: StateStoreWriteTransaction>(
        tx: &mut TTx,
        block_id: &BlockId,
        status: ForeignProposalStatus,
        set_proposed_in_block: Option<&BlockId>,
    ) -> Result<(), StorageError> {
        tx.foreign_proposals_set_status(block_id, status, set_proposed_in_block)
    }

    pub fn delete<TTx: StateStoreWriteTransaction>(tx: &mut TTx, block_id: &BlockId) -> Result<(), StorageError> {
        tx.foreign_proposals_delete(block_id)
    }

    pub fn delete_in_epoch<TTx: StateStoreWriteTransaction>(tx: &mut TTx, epoch: Epoch) -> Result<(), StorageError> {
        tx.foreign_proposals_delete_in_epoch(epoch)
    }

    pub fn get_any<'a, TTx: StateStoreReadTransaction, I: IntoIterator<Item = &'a BlockId>>(
        tx: &TTx,
        block_ids: I,
    ) -> Result<Vec<Self>, StorageError> {
        tx.foreign_proposals_get_any(block_ids)
    }

    pub fn exists<TTx: StateStoreReadTransaction>(&self, tx: &TTx) -> Result<bool, StorageError> {
        Self::record_exists(tx, self.block_id())
    }

    pub fn record_exists<TTx: StateStoreReadTransaction>(tx: &TTx, block_id: &BlockId) -> Result<bool, StorageError> {
        tx.foreign_proposals_exists(block_id)
    }

    pub fn get_all_new<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        block_id: &BlockId,
        limit: usize,
    ) -> Result<Vec<Self>, StorageError> {
        tx.foreign_proposals_get_all_new(block_id, limit)
    }

    pub fn has_unconfirmed<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch) -> Result<bool, StorageError> {
        tx.foreign_proposals_has_unconfirmed(epoch)
    }
}

impl Display for ForeignProposalRecord {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ForeignProposalRecord({}, {}, cmds: {}, plg: {}, {}, {})",
            self.block_id,
            self.shard_group_unchecked(),
            self.proposal.commit_proof().commands().len(),
            self.proposal.block_pledge().len(),
            self.status,
            self.proposed_in_block
                .as_ref()
                .map_or_else(|| "None".to_string(), |id| id.to_string())
        )
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, PartialOrd, Ord, BorshSerialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct ForeignProposalAtom {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub block_id: BlockId,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub shard_group: ShardGroup,
}

impl ForeignProposalAtom {
    pub fn exists<TTx: StateStoreReadTransaction>(&self, tx: &TTx) -> Result<bool, StorageError> {
        tx.foreign_proposals_exists(&self.block_id)
    }

    pub fn get_proposal<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
    ) -> Result<ForeignProposalRecord, StorageError> {
        let mut found = tx.foreign_proposals_get_any(Some(&self.block_id))?;
        let found = found.pop().ok_or_else(|| StorageError::NotFound {
            item: "ForeignProposal",
            key: self.block_id.to_string(),
        })?;
        Ok(found)
    }

    pub fn delete<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        ForeignProposalRecord::delete(tx, &self.block_id)
    }

    pub fn set_status<TTx: StateStoreWriteTransaction>(
        &self,
        tx: &mut TTx,
        status: ForeignProposalStatus,
        set_proposed_in: Option<&BlockId>,
    ) -> Result<(), StorageError> {
        tx.foreign_proposals_set_status(&self.block_id, status, set_proposed_in)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, Eq, PartialEq)]
pub enum ForeignProposalStatus {
    /// New foreign proposal that has not yet been proposed
    #[default]
    New,
    /// Foreign proposal has been proposed, but not yet locked.
    Proposed,
    /// Foreign proposal has been confirmed i.e. the block containing it has been locked.
    Confirmed,
    /// Foreign proposal has been rejected.
    Invalid,
}

impl ForeignProposalStatus {
    pub fn is_new(&self) -> bool {
        matches!(self, ForeignProposalStatus::New)
    }

    pub fn is_proposed(&self) -> bool {
        matches!(self, ForeignProposalStatus::Proposed)
    }

    pub fn is_confirmed(&self) -> bool {
        matches!(self, ForeignProposalStatus::Confirmed)
    }

    pub fn is_invalid(&self) -> bool {
        matches!(self, ForeignProposalStatus::Invalid)
    }

    pub fn is_unconfirmed(&self) -> bool {
        matches!(self, ForeignProposalStatus::New | ForeignProposalStatus::Proposed)
    }
}

impl Display for ForeignProposalStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ForeignProposalStatus::New => write!(f, "New"),
            ForeignProposalStatus::Proposed => write!(f, "Proposed"),
            ForeignProposalStatus::Confirmed => write!(f, "Confirmed"),
            ForeignProposalStatus::Invalid => write!(f, "Invalid"),
        }
    }
}

impl FromStr for ForeignProposalStatus {
    type Err = StorageError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "New" => Ok(ForeignProposalStatus::New),
            "Proposed" => Ok(ForeignProposalStatus::Proposed),
            "Confirmed" => Ok(ForeignProposalStatus::Confirmed),
            "Invalid" => Ok(ForeignProposalStatus::Invalid),
            _ => Err(StorageError::DecodingError {
                operation: "ForeignProposalStatus::from_str",
                item: "foreign proposal",
                details: format!("Invalid foreign proposal state {}", s),
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignProposal {
    commit_proof: CommandsCommitProof,
    block_pledge: BlockPledge,
}

impl ForeignProposal {
    pub fn new(commit_proof: CommandsCommitProof, block_pledge: BlockPledge) -> Self {
        Self {
            commit_proof,
            block_pledge,
        }
    }

    pub fn to_atom(&self) -> ForeignProposalAtom {
        ForeignProposalAtom {
            shard_group: self.shard_group_unchecked(),
            block_id: self
                .commit_proof
                .sidechain_block_commit_proof()
                .header
                .calculate_block_id()
                .into(),
        }
    }

    pub fn commit_proof(&self) -> &CommandsCommitProof {
        &self.commit_proof
    }

    pub fn block_pledge(&self) -> &BlockPledge {
        &self.block_pledge
    }

    pub fn get_justify_qc(&self) -> Option<&QuorumCertificate> {
        self.commit_proof
            .sidechain_block_commit_proof()
            .proof_elements
            // Should be the last element
            .last()
            .and_then(|elem| {
                if let CommitProofElement::QuorumCertificate(qc) = elem {
                    Some(qc)
                } else {
                    None
                }
            })
    }

    pub fn epoch(&self) -> Epoch {
        Epoch(self.commit_proof.sidechain_block_commit_proof().header.epoch)
    }

    pub fn height(&self) -> NodeHeight {
        NodeHeight(self.commit_proof.sidechain_block_commit_proof().header.height)
    }

    pub fn proposed_by(&self) -> RistrettoPublicKeyBytes {
        RistrettoPublicKeyBytes::from_bytes(
            self.commit_proof
                .sidechain_block_commit_proof()
                .header
                .proposed_by
                .as_bytes(),
        )
        .expect("CompressedPublicKey is not 32 bytes")
    }

    pub fn network_byte(&self) -> u8 {
        self.commit_proof.sidechain_block_commit_proof().header.network
    }

    pub fn shard_group_checked(&self) -> Option<ShardGroup> {
        ShardGroup::new_checked(
            self.commit_proof
                .sidechain_block_commit_proof()
                .header
                .shard_group
                .start,
            self.commit_proof
                .sidechain_block_commit_proof()
                .header
                .shard_group
                .end_inclusive,
        )
    }

    pub fn shard_group_unchecked(&self) -> ShardGroup {
        ShardGroup::new_unchecked(
            self.commit_proof
                .sidechain_block_commit_proof()
                .header
                .shard_group
                .start,
            self.commit_proof
                .sidechain_block_commit_proof()
                .header
                .shard_group
                .end_inclusive,
        )
    }

    pub fn calculate_block_id(&self) -> BlockId {
        self.commit_proof
            .sidechain_block_commit_proof()
            .header
            .calculate_block_id()
            .into()
    }

    pub fn all_transaction_ids_in_committee<'a>(
        &'a self,
        committee_info: &'a CommitteeInfo,
    ) -> impl Iterator<Item = &'a TransactionId> + Clone + 'a {
        self.commit_proof
            .commands()
            .iter()
            .filter_map(|cmd| cmd.command())
            .filter_map(|cmd| cmd.transaction())
            .filter(|t| t.evidence.has_and_not_empty(&committee_info.shard_group()))
            .map(|t| t.id())
    }

    pub fn commands(&self) -> &[CommandOrHash] {
        self.commit_proof().commands()
    }

    pub fn full_commands_iter(&self) -> impl Iterator<Item = &Command> + '_ {
        self.commands().iter().filter_map(|cmd| cmd.command())
    }

    pub fn into_parts(self) -> (CommandsCommitProof, BlockPledge) {
        (self.commit_proof, self.block_pledge)
    }
}

impl Display for ForeignProposal {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ForeignProposal({}, {} command(s), {} pledge(s))",
            self.commit_proof.calculate_block_id(),
            self.commit_proof.commands().len(),
            self.block_pledge.len()
        )
    }
}
