//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_ootle_common_types::{committee::CommitteeInfo, optional::Optional};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    consensus_models::{
        Block,
        Command,
        LocalOnlyAtom,
        MultiShardAtom,
        NoVoteReason,
        TransactionPoolRecord,
        TransactionPoolStage,
    },
};

use crate::{
    hotstuff::{
        block_change_set::ProposedBlockChangeSet,
        command_derivation::{
            DerivedCommand,
            EvidenceMatch,
            MultiShardCommandKind,
            as_new_multishard_command,
            local_only_disagreement,
            locked_epoch_disagreement,
            multishard_disagreement,
            new_multishard_disagreement,
        },
        command_sequencer::{BlockTotals, CommandSequencer, Sequencing},
        error::HotStuffError,
        substate_store::PendingSubstateStore,
        transaction_manager::ConsensusTransactionManager,
    },
    tracing::TraceTimer,
    traits::BlockTransactionExecutor,
};

// Logs under the target of the handler that drives these checks, so operators filter one target for every no-vote.
const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_ready_to_vote_on_local_block";

/// Decides whether a replica can vote for each transaction command in a block proposed by its own committee. Each
/// command is re-derived with the [`CommandSequencer`] the proposer uses, from this node's own pool record, and voted
/// for only if the proposer's command equals it.
pub(super) struct LocalCommandValidator<'a, TExecutor, TStateStore> {
    transaction_manager: &'a ConsensusTransactionManager<TExecutor, TStateStore>,
    local_validator_pk: &'a RistrettoPublicKey,
}

impl<'a, TExecutor, TStateStore> LocalCommandValidator<'a, TExecutor, TStateStore>
where
    TStateStore: StateStore,
    TExecutor: BlockTransactionExecutor<TStateStore>,
{
    pub fn new(
        transaction_manager: &'a ConsensusTransactionManager<TExecutor, TStateStore>,
        local_validator_pk: &'a RistrettoPublicKey,
    ) -> Self {
        Self {
            transaction_manager,
            local_validator_pk,
        }
    }

    pub fn evaluate_local_only_command<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        block: &Block,
        atom: &LocalOnlyAtom,
        local_committee_info: &CommitteeInfo,
        substate_store: &mut PendingSubstateStore<TTx>,
        change_set: &mut ProposedBlockChangeSet,
        totals: &mut BlockTotals,
    ) -> Result<Option<NoVoteReason>, HotStuffError> {
        let _timer = TraceTimer::info(LOG_TARGET, "Evaluate LocalOnly command");
        let pool_record = match self.pool_record_at_stage(
            tx,
            block,
            atom.id(),
            TransactionPoolStage::New,
            "LocalOnly",
            substate_store,
            change_set,
        )? {
            Ok(pool_record) => pool_record,
            Err(reason) => return Ok(Some(reason)),
        };

        let sequencer = CommandSequencer::for_block(self.transaction_manager, local_committee_info, block);
        let sequencing = sequencer.sequence_new(pool_record, "LocalOnly", substate_store, change_set)?;
        let derived = match refused(sequencing, block, atom.id(), "LocalOnly") {
            Ok(derived) => derived,
            Err(reason) => return Ok(Some(reason)),
        };
        let Command::LocalOnly(derived_atom) = &derived.command else {
            warn!(
                target: LOG_TARGET,
                "❌ NO VOTE: transaction {} in block {block} is not Local-Only but was proposed as LocalOnly",
                atom.id(),
            );
            return Ok(Some(NoVoteReason::LocalOnlyProposedForMultiShard));
        };
        if let Some(reason) = local_only_disagreement(derived_atom, atom) {
            warn!(
                target: LOG_TARGET,
                "❌ NO VOTE: {reason} for LocalOnly transaction {} in block {block}. Leader proposed {atom}, we derived \
                 {derived_atom}",
                atom.id(),
            );
            return Ok(Some(reason));
        }

        sequencer.commit(derived, change_set, totals)?;
        Ok(None)
    }

    pub fn evaluate_local_prepare_command<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        block: &Block,
        atom: &MultiShardAtom,
        local_committee_info: &CommitteeInfo,
        substate_store: &mut PendingSubstateStore<TTx>,
        change_set: &mut ProposedBlockChangeSet,
        totals: &mut BlockTotals,
    ) -> Result<Option<NoVoteReason>, HotStuffError> {
        let pool_record = match self.pool_record_at_stage(
            tx,
            block,
            atom.id(),
            TransactionPoolStage::New,
            "LocalPrepare",
            substate_store,
            change_set,
        )? {
            Ok(pool_record) => pool_record,
            Err(reason) => return Ok(Some(reason)),
        };

        self.evaluate_new_multishard_command(
            pool_record,
            block,
            MultiShardCommandKind::LocalPrepare,
            atom,
            local_committee_info,
            substate_store,
            change_set,
            totals,
        )
    }

    pub fn evaluate_local_accept_command<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        block: &Block,
        atom: &MultiShardAtom,
        local_committee_info: &CommitteeInfo,
        next_epoch_changes_layout: bool,
        substate_store: &mut PendingSubstateStore<TTx>,
        change_set: &mut ProposedBlockChangeSet,
        totals: &mut BlockTotals,
    ) -> Result<Option<NoVoteReason>, HotStuffError> {
        let Some(pool_record) = self.pool_record(tx, block, atom.id(), substate_store, change_set)? else {
            return Ok(Some(NoVoteReason::TransactionNotInPool));
        };

        if pool_record.current_stage().is_new() {
            // Sequenced straight to LocalAccept, which can only mean either the transaction aborts or this shard
            // group only creates its outputs
            return self.evaluate_new_multishard_command(
                pool_record,
                block,
                MultiShardCommandKind::LocalAccept,
                atom,
                local_committee_info,
                substate_store,
                change_set,
                totals,
            );
        }

        if let Some(reason) = locked_epoch_disagreement(pool_record.locked_epoch(), atom) {
            warn!(target: LOG_TARGET, "❌ NO VOTE LocalAccept in block {block}: {reason}");
            return Ok(Some(reason));
        }
        if let Some(reason) =
            self.stage_disagreement(&pool_record, block, TransactionPoolStage::LocalPrepared, "LocalAccept")
        {
            return Ok(Some(reason));
        }

        let sequencer = CommandSequencer::for_block(self.transaction_manager, local_committee_info, block);
        // A replica only accepts the epoch-end abort of a transaction still waiting on foreign shard groups when the
        // leader proposed it, because an honest leader proposes it only when its own view of the next epoch permits
        let abort_waiting_for_epoch_end = next_epoch_changes_layout && atom.decision.is_abort();
        let sequencing =
            sequencer.sequence_local_accept(pool_record, abort_waiting_for_epoch_end, substate_store, change_set)?;
        self.compare_and_commit(
            &sequencer,
            sequencing,
            block,
            atom,
            "LocalAccept",
            EvidenceMatch::Pledges,
            change_set,
            totals,
        )
    }

    pub fn evaluate_all_accept_command<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        block: &Block,
        atom: &MultiShardAtom,
        local_committee_info: &CommitteeInfo,
        substate_store: &mut PendingSubstateStore<TTx>,
        change_set: &mut ProposedBlockChangeSet,
        totals: &mut BlockTotals,
    ) -> Result<Option<NoVoteReason>, HotStuffError> {
        if atom.decision.is_abort() {
            warn!(
                target: LOG_TARGET,
                "❌ NO VOTE: AllAccept command received for block {} but requires that the transaction is COMMIT",
                block.id(),
            );
            return Ok(Some(NoVoteReason::AllAcceptMustBeCommit {
                transaction_id: atom.id,
                block_id: *block.id(),
            }));
        }
        self.evaluate_accept_command(
            tx,
            block,
            atom,
            "AllAccept",
            local_committee_info,
            substate_store,
            change_set,
            totals,
        )
    }

    pub fn evaluate_some_accept_command<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        block: &Block,
        atom: &MultiShardAtom,
        local_committee_info: &CommitteeInfo,
        substate_store: &mut PendingSubstateStore<TTx>,
        change_set: &mut ProposedBlockChangeSet,
        totals: &mut BlockTotals,
    ) -> Result<Option<NoVoteReason>, HotStuffError> {
        if atom.decision.is_commit() {
            warn!(
                target: LOG_TARGET,
                "❌ SomeAccept command received for block {} but requires that the atom is ABORT",
                block.id(),
            );
            return Ok(Some(NoVoteReason::SomeAcceptAtomMustBeAbort {
                transaction_id: atom.id,
                block_id: *block.id(),
            }));
        }
        self.evaluate_accept_command(
            tx,
            block,
            atom,
            "SomeAccept",
            local_committee_info,
            substate_store,
            change_set,
            totals,
        )
    }

    fn evaluate_accept_command<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        block: &Block,
        atom: &MultiShardAtom,
        command: &'static str,
        local_committee_info: &CommitteeInfo,
        substate_store: &mut PendingSubstateStore<TTx>,
        change_set: &mut ProposedBlockChangeSet,
        totals: &mut BlockTotals,
    ) -> Result<Option<NoVoteReason>, HotStuffError> {
        let pool_record = match self.pool_record_at_stage(
            tx,
            block,
            atom.id(),
            TransactionPoolStage::LocalAccepted,
            command,
            substate_store,
            change_set,
        )? {
            Ok(pool_record) => pool_record,
            Err(reason) => return Ok(Some(reason)),
        };

        let sequencer = CommandSequencer::for_block(self.transaction_manager, local_committee_info, block);
        let sequencing = sequencer.sequence_accept(pool_record, substate_store, change_set)?;
        self.compare_and_commit(
            &sequencer,
            sequencing,
            block,
            atom,
            command,
            EvidenceMatch::Exact,
            change_set,
            totals,
        )
    }

    /// Evaluates a `proposed_kind` command proposed for a transaction at the New stage.
    fn evaluate_new_multishard_command<TTx: StateStoreReadTransaction>(
        &self,
        pool_record: TransactionPoolRecord,
        block: &Block,
        proposed_kind: MultiShardCommandKind,
        atom: &MultiShardAtom,
        local_committee_info: &CommitteeInfo,
        substate_store: &mut PendingSubstateStore<TTx>,
        change_set: &mut ProposedBlockChangeSet,
        totals: &mut BlockTotals,
    ) -> Result<Option<NoVoteReason>, HotStuffError> {
        let _timer = TraceTimer::info(LOG_TARGET, proposed_kind.as_str());
        info!(
            target: LOG_TARGET,
            "👨‍🔧 {proposed_kind}: Transaction {} in block {block}",
            pool_record.id(),
        );

        let sequencer = CommandSequencer::for_block(self.transaction_manager, local_committee_info, block);
        let sequencing = sequencer.sequence_new(pool_record, proposed_kind.as_str(), substate_store, change_set)?;
        let derived = match refused(sequencing, block, atom.id(), proposed_kind.as_str()) {
            Ok(derived) => derived,
            Err(reason) => return Ok(Some(reason)),
        };
        let Some((derived_kind, derived_atom)) = as_new_multishard_command(&derived.command) else {
            warn!(
                target: LOG_TARGET,
                "❌ NO VOTE: transaction {} in block {block} is Local-Only but was proposed as {proposed_kind}",
                atom.id(),
            );
            return Ok(Some(NoVoteReason::MultiShardProposedForLocalOnly));
        };
        if let Some(reason) = new_multishard_disagreement(derived_kind, derived_atom, proposed_kind, atom) {
            warn!(
                target: LOG_TARGET,
                "❌ NO VOTE: {reason} for transaction {} in block {block}. Leader proposed {proposed_kind} {atom}, we \
                 derived {}",
                atom.id(),
                derived.command,
            );
            return Ok(Some(reason));
        }

        sequencer.commit(derived, change_set, totals)?;
        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    fn compare_and_commit(
        &self,
        sequencer: &CommandSequencer<'_, TExecutor, TStateStore>,
        sequencing: Sequencing,
        block: &Block,
        atom: &MultiShardAtom,
        command: &'static str,
        evidence_match: EvidenceMatch,
        change_set: &mut ProposedBlockChangeSet,
        totals: &mut BlockTotals,
    ) -> Result<Option<NoVoteReason>, HotStuffError> {
        let derived = match refused(sequencing, block, atom.id(), command) {
            Ok(derived) => derived,
            Err(reason) => return Ok(Some(reason)),
        };
        let derived_atom = match &derived.command {
            Command::LocalAccept(atom) | Command::AllAccept(atom) | Command::SomeAccept(atom) => atom,
            derived_command => {
                return Err(HotStuffError::InvariantError(format!(
                    "{command} of transaction {} derived {derived_command}",
                    atom.id(),
                )));
            },
        };
        if let Some(reason) = multishard_disagreement(derived_atom, atom, evidence_match) {
            warn!(
                target: LOG_TARGET,
                "❌ NO VOTE: {reason} for {command} of transaction {} in block {block}. Leader proposed {atom}, we \
                 derived {}",
                atom.id(),
                derived.command,
            );
            return Ok(Some(reason));
        }

        sequencer.commit(derived, change_set, totals)?;
        Ok(None)
    }

    fn pool_record<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        block: &Block,
        transaction_id: &tari_ootle_transaction::TransactionId,
        substate_store: &PendingSubstateStore<TTx>,
        change_set: &ProposedBlockChangeSet,
    ) -> Result<Option<TransactionPoolRecord>, HotStuffError> {
        let pool_record = change_set
            .get_transaction_pool_record(tx, substate_store.parent_chain(), transaction_id)
            .optional()?;
        if pool_record.is_none() {
            warn!(
                target: LOG_TARGET,
                "⚠️ Local proposal received ({block}) for transaction {transaction_id} which is not in the pool. This \
                 is likely a previous transaction that has been re-proposed. Not voting on block.",
            );
        }
        Ok(pool_record)
    }

    /// The pool record of `transaction_id` if it is at the `expected` stage, or why `command` is refused.
    #[allow(clippy::too_many_arguments)]
    fn pool_record_at_stage<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        block: &Block,
        transaction_id: &tari_ootle_transaction::TransactionId,
        expected: TransactionPoolStage,
        command: &'static str,
        substate_store: &PendingSubstateStore<TTx>,
        change_set: &ProposedBlockChangeSet,
    ) -> Result<Result<TransactionPoolRecord, NoVoteReason>, HotStuffError> {
        let Some(pool_record) = self.pool_record(tx, block, transaction_id, substate_store, change_set)? else {
            return Ok(Err(NoVoteReason::TransactionNotInPool));
        };
        match self.stage_disagreement(&pool_record, block, expected, command) {
            Some(reason) => Ok(Err(reason)),
            None => Ok(Ok(pool_record)),
        }
    }

    fn stage_disagreement(
        &self,
        pool_record: &TransactionPoolRecord,
        block: &Block,
        expected: TransactionPoolStage,
        command: &'static str,
    ) -> Option<NoVoteReason> {
        let stage = pool_record.current_stage();
        if stage == expected {
            return None;
        }
        warn!(
            target: LOG_TARGET,
            "{} ❌ {command} stage disagreement in block {block} for transaction {}. Expected {expected}, local stage \
             is {stage}",
            self.local_validator_pk,
            pool_record.id(),
        );
        Some(NoVoteReason::StageDisagreement { expected, stage })
    }
}

/// The command derived for `transaction_id`, or why the sequencer refused it.
fn refused(
    sequencing: Sequencing,
    block: &Block,
    transaction_id: &tari_ootle_transaction::TransactionId,
    command: &'static str,
) -> Result<DerivedCommand, NoVoteReason> {
    sequencing.map_err(|refusal| {
        warn!(
            target: LOG_TARGET,
            "❌ NO VOTE: {command} of transaction {transaction_id} in block {block} cannot be sequenced: {}",
            refusal.reason,
        );
        refusal.reason
    })
}

#[cfg(test)]
mod tests;
