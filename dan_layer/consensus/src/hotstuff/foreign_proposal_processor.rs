//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_dan_common_types::{
    committee::CommitteeInfo,
    option::Displayable,
    optional::Optional,
    ShardGroup,
    SubstateAddress,
};
use tari_dan_storage::{
    consensus_models::{
        BlockPledge,
        Command,
        Decision,
        ForeignProposal,
        LeafBlock,
        LockedBlock,
        LockedSubstateValue,
        TransactionAtom,
        TransactionPoolRecord,
        TransactionPoolStage,
        TransactionRecord,
    },
    StateStore,
    StateStoreReadTransaction,
};
use tari_engine_types::commit_result::RejectReason;

use crate::{
    hotstuff::{
        block_change_set::ProposedBlockChangeSet,
        error::HotStuffError,
        substate_store::PendingSubstateStore,
        ProposalValidationError,
    },
    tracing::TraceTimer,
};

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::foreign_proposal_processor";

#[allow(clippy::too_many_lines)]
pub fn process_foreign_block<TStore: StateStore>(
    tx: &TStore::ReadTransaction<'_>,
    local_leaf: &LeafBlock,
    locked_block: &LockedBlock,
    proposal: &ForeignProposal,
    local_committee_info: &CommitteeInfo,
    substate_store: &mut PendingSubstateStore<TStore>,
    proposed_block_change_set: &mut ProposedBlockChangeSet,
) -> Result<(), HotStuffError> {
    let _timer = TraceTimer::info(LOG_TARGET, "process_foreign_block");

    let foreign_shard_group = proposal.block.shard_group();
    assert_eq!(
        proposal.block.shard_group(),
        foreign_shard_group,
        "Foreign proposal shard group does not match the foreign committee shard group"
    );
    info!(
        target: LOG_TARGET,
        "🧩 Processing FOREIGN PROPOSAL {}, justify_qc: {}",
        proposal.block(),
        proposal.justify_qc(),
    );

    let ForeignProposal {
        ref block,
        ref justify_qc,
        ref block_pledge,
        ..
    } = proposal;
    let mut command_count = 0usize;

    for cmd in block.commands() {
        match cmd {
            Command::LocalPrepare(atom) => {
                if !atom.evidence.has(&local_committee_info.shard_group()) ||
                    // Foreign output-only nodes should not do a local prepare, if they do we ignore it
                    atom.evidence
                        .is_committee_output_only(foreign_shard_group)
                {
                    debug!(
                        target: LOG_TARGET,
                        "🧩 FOREIGN PROPOSAL: Command: LocalPrepare({}, {}), block: {} not relevant to local committee",
                        atom.id, atom.decision, block.id(),
                    );
                    continue;
                }

                debug!(
                    target: LOG_TARGET,
                    "🧩 FOREIGN PROPOSAL: Command: LocalPrepare({}, {}), block: {}",
                    atom.id,atom.decision, block.id(),
                );

                let Some(mut tx_rec) = proposed_block_change_set
                    .get_transaction(tx, locked_block, local_leaf, &atom.id)
                    .optional()?
                else {
                    // CASE: the transaction was already aborted by this node when locking outputs (LocalAccept) and
                    // the transaction was finalized and therefore not in the pool.
                    if TransactionRecord::exists(tx, &atom.id)? {
                        info!(
                            target: LOG_TARGET,
                            "❓️Foreign proposal {} received for transaction {} but this transaction is already (presumably) finalized.",
                            block.id(),
                            atom.id
                        );
                    } else {
                        // Might be a bug in the foreign missing transaction handling
                        warn!(
                            target: LOG_TARGET,
                            "⚠️ NEVER HAPPEN: Foreign proposal {} received for transaction {} but this transaction is not in the pool.",
                            block.id(),
                            atom.id
                        );
                    }
                    continue;
                };

                command_count += 1;

                if tx_rec.current_stage() > TransactionPoolStage::LocalPrepared {
                    // CASE: This will happen if output-only nodes send a prepare to input-involved nodes.
                    warn!(
                        target: LOG_TARGET,
                        "⚠️ Foreign LocalPrepare proposal ({}) received LOCAL_PREPARE for transaction {} but current transaction stage is {}. Ignoring.",
                        block,
                        tx_rec.transaction_id(), tx_rec.current_stage()
                    );
                    continue;
                }

                let remote_decision = atom.decision;
                let local_decision = tx_rec.current_decision();
                if let Some(abort_reason) = remote_decision.abort_reason() {
                    if local_decision.is_commit() {
                        info!(
                            target: LOG_TARGET,
                            "⚠️ Foreign committee ABORT transaction {}. Update overall decision to ABORT. Local stage: {}, Leaf: {}",
                            tx_rec.transaction_id(), tx_rec.current_stage(), local_leaf
                        );

                        // Add an abort execution since we previously decided to commit
                        let mut transaction = TransactionRecord::get(tx, tx_rec.transaction_id())?;
                        transaction.abort(RejectReason::ForeignShardGroupDecidedToAbort {
                            start_shard: foreign_shard_group.start().as_u32(),
                            end_shard: foreign_shard_group.end().as_u32(),
                            abort_reason: abort_reason.to_string(),
                        });
                        tx_rec.set_local_decision(transaction.current_decision());
                        let exec = transaction.into_execution().expect("ABORT set above");
                        proposed_block_change_set.add_transaction_execution(exec)?;
                    }
                }

                // Update the transaction record with any new information provided by this foreign block
                let Some(foreign_evidence) = atom.evidence.get(&foreign_shard_group) else {
                    return Err(ProposalValidationError::ForeignInvalidPledge {
                        block: block.as_leaf_block(),
                        transaction_id: atom.id,
                        shard_group: foreign_shard_group,
                        details: "Foreign proposal did not contain evidence for it's own shard group".to_string(),
                    }
                    .into());
                };

                tx_rec
                    .evidence_mut()
                    .add_shard_group(foreign_shard_group)
                    .update(foreign_evidence)
                    .set_prepare_qc(*justify_qc.id());
                tx_rec.set_remote_decision(remote_decision);

                // CASE: local node has pledged a substate S to tx_1 and foreign node has locked the same substate S to
                // tx_2. If the local node has already prepared the transaction, we must abort tx_2. If
                // they have received our localprepare, they will abort tx_1 and we'll both abort.

                let inputs = tx_rec
                    .evidence()
                    .get(&local_committee_info.shard_group())
                    .map(|ev| ev.inputs().keys())
                    .into_iter()
                    .flatten();

                // If we have not yet prepared the transaction, we can wait to do that until this transaction is
                // finalised and there is no need to abort either. If we have already prepared the transaction, we
                // need to abort in this case since we've already pledged.
                if tx_rec.current_decision().is_commit() {
                    if let Some(conflicting_transaction_id) =
                        LockedSubstateValue::get_transaction_id_that_conflicts_with_write_locks(
                            substate_store.read_transaction(),
                            tx_rec.transaction_id(),
                            inputs,
                            false,
                        )?
                    {
                        // Determine if we're the only conflicting shard group. If so, we can ignore this conflict
                        // and wait for the conflicting transaction to be finalised before pledging, using the outputs
                        // of this transaction. If not, resolving the conflict is more
                        // complex/unsafe, and we need to abort.
                        let conflicting_tx = proposed_block_change_set.get_transaction(
                            tx,
                            locked_block,
                            local_leaf,
                            &conflicting_transaction_id,
                        )?;
                        let has_conflicts = conflicting_tx
                            .evidence()
                            .iter()
                            .filter(|(sg, _)| **sg != local_committee_info.shard_group())
                            .any(|(sg, conflicting_evidence)| {
                                tx_rec.evidence().get(sg).is_some_and(|shard_ev| {
                                    shard_ev.inputs().iter().any(|(id, e)| {
                                        // If the current transaction (tx_rec) has a strict input version, it will be
                                        // aborted later.
                                        let conflicting_is_write = e.as_ref().map_or(true, |e| e.is_write);
                                        conflicting_evidence.inputs().iter().any(|(input_id, input)| {
                                            // Are either write?
                                            (conflicting_is_write || input.as_ref().map_or(true, |e| e.is_write)) &&
                                                input_id == id
                                        })
                                    })
                                })
                            });
                        if has_conflicts {
                            warn!(
                                target: LOG_TARGET,
                                "⚠️ Foreign proposal {} received for transaction {} but a conflicting transaction ({}) has already been prepared by this node. Abort.",
                                block,
                                tx_rec.transaction_id(),
                                conflicting_transaction_id
                            );

                            // Add an abort execution since we previously decided to commit
                            let mut transaction = TransactionRecord::get(tx, tx_rec.transaction_id())?;
                            transaction.abort(RejectReason::ForeignPledgeInputConflict);
                            tx_rec.set_local_decision(transaction.current_decision());
                            let exec = transaction.into_execution().expect("ABORT set above");
                            proposed_block_change_set.add_transaction_execution(exec)?;
                        } else {
                            info!(
                                target: LOG_TARGET,
                                "Transaction {} conflicts with {} but does not conflict with any foreign pledges. No need to abort.",
                                tx_rec.transaction_id(),
                                conflicting_transaction_id
                            );
                        }
                    }
                }

                add_pledges(
                    &tx_rec,
                    block.as_leaf_block(),
                    atom,
                    block_pledge,
                    foreign_shard_group,
                    local_committee_info.shard_group(),
                    proposed_block_change_set,
                    true,
                )?;

                // tx_rec.evidence().iter().for_each(|(addr, ev)| {
                //     let includes_local = local_committee_info.includes_substate_address(addr);
                //     log::error!(
                //         target: LOG_TARGET,
                //         "🐞 LOCALPREPARE EVIDENCE (l={}, f={}) {}: {}", includes_local, !includes_local, addr, ev
                //     );
                // });

                if tx_rec.current_stage().is_new() {
                    info!(
                        target: LOG_TARGET,
                        "🧩 FOREIGN PROPOSAL: (Initial sequence from LocalPrepare) Transaction is ready for Prepare({}, {}) Local Stage: {}",
                        tx_rec.transaction_id(),
                        tx_rec.current_decision(),
                        tx_rec.current_stage()
                    );
                    // If the transaction is New, we're waiting for all foreign pledges. Propose transaction once we
                    // have them.

                    // CASE: One foreign SG is involved in all inputs and executed the transaction, local SG is
                    // involved in the outputs
                    let is_ready = tx_rec.current_decision().is_abort() ||
                        // local_committee_info.includes_substate_id(&tx_rec.to_receipt_id().into()) ||
                        tx_rec.committee_involves_inputs(local_committee_info) ||
                        has_all_foreign_input_pledges(tx, &tx_rec, local_committee_info, proposed_block_change_set)?;

                    if is_ready {
                        info!(
                            target: LOG_TARGET,
                            "🧩 FOREIGN PROPOSAL: (Initial sequence from LocalPrepare) Transaction is ready for Prepare({}, {}) Local Stage: {}",
                            tx_rec.transaction_id(),
                            tx_rec.current_decision(),
                            tx_rec.current_stage()
                        );
                        tx_rec.set_ready(true);
                        tx_rec.set_next_stage(TransactionPoolStage::New)?;
                        proposed_block_change_set.set_next_transaction_update(tx_rec)?;
                    } else {
                        info!(
                            target: LOG_TARGET,
                            "🧩 FOREIGN PROPOSAL: (Initial sequence from LocalPrepare) Transaction is NOT ready for Prepare({}, {}) Local Stage: {}",
                            tx_rec.transaction_id(),
                            tx_rec.current_decision(),
                            tx_rec.current_stage()
                        );
                    }
                } else if tx_rec.current_stage().is_local_prepared() &&
                    tx_rec.evidence().all_input_shard_groups_prepared()
                {
                    // If all shards are complete, and we've already received our LocalPrepared, we can set the
                    // LocalPrepared transaction as ready to propose AllPrepared. If we have not received
                    // the local LocalPrepared, the transition to AllPrepared will occur after we receive the local
                    // LocalPrepare proposal.
                    info!(
                        target: LOG_TARGET,
                        "🧩 FOREIGN PROPOSAL: Transaction is ready for propose AllPrepared({}, {}) Local Stage: {}",
                        tx_rec.transaction_id(),
                        tx_rec.current_decision(),
                        tx_rec.current_stage()
                    );

                    tx_rec.set_next_stage(TransactionPoolStage::LocalPrepared)?;
                    proposed_block_change_set.set_next_transaction_update(tx_rec)?;
                } else {
                    info!(
                        target: LOG_TARGET,
                        "🧩 FOREIGN PROPOSAL: Transaction is NOT ready for AllPrepared({}, {}) Local Stage: {}, \
                        All Justified: {}. Waiting for local proposal and/or additional foreign proposals for all other shard groups.",
                        tx_rec.transaction_id(),
                        tx_rec.current_decision(),
                        tx_rec.current_stage(),
                         tx_rec.evidence().all_input_shard_groups_prepared()
                    );
                    // Update the evidence
                    proposed_block_change_set.set_next_transaction_update(tx_rec)?;
                }
            },
            Command::LocalAccept(atom) => {
                if !atom.evidence.has(&local_committee_info.shard_group()) {
                    continue;
                }

                debug!(
                    target: LOG_TARGET,
                    "🧩 FOREIGN PROPOSAL: Command: LocalAccept({}, {}), block: {}",
                    atom.id, atom.decision, block.id(),
                );

                let Some(mut tx_rec) = proposed_block_change_set
                    .get_transaction(tx, locked_block, local_leaf, &atom.id)
                    .optional()?
                else {
                    if TransactionRecord::exists(tx, &atom.id)? {
                        info!(
                            target: LOG_TARGET,
                            "❓️Foreign proposal {} received for transaction {} but this transaction is already (presumably) finalized.",
                            block.id(),
                            atom.id
                        );
                    } else {
                        warn!(
                            target: LOG_TARGET,
                            "⚠️ NEVER HAPPEN: Foreign proposal {} received for transaction {} but this transaction is not in the pool.",
                            block.id(),
                            atom.id
                        );
                    }
                    continue;
                };

                command_count += 1;

                if tx_rec.current_stage() > TransactionPoolStage::LocalAccepted {
                    warn!(
                        target: LOG_TARGET,
                        "⚠️ Foreign proposal {} received LOCAL_ACCEPT for transaction {} but current transaction stage is {}. Ignoring.",
                        block,
                        tx_rec.transaction_id(),
                        tx_rec.current_stage(),
                    );
                    continue;
                }

                let remote_decision = atom.decision;
                let local_decision = tx_rec.current_decision();
                if let Some(abort_reason) = remote_decision.abort_reason() {
                    if local_decision.is_commit() {
                        info!(
                            target: LOG_TARGET,
                            "⚠️ Foreign ABORT {}. Update overall decision to ABORT. Local stage: {}, Leaf: {}",
                            tx_rec.transaction_id(), tx_rec.current_stage(), local_leaf
                        );
                        // Add an abort execution since we previously decided to commit
                        let mut transaction = TransactionRecord::get(tx, tx_rec.transaction_id())?;
                        transaction.abort(RejectReason::ForeignShardGroupDecidedToAbort {
                            start_shard: foreign_shard_group.start().as_u32(),
                            end_shard: foreign_shard_group.end().as_u32(),
                            abort_reason: abort_reason.to_string(),
                        });
                        tx_rec.set_local_decision(transaction.current_decision());
                        let exec = transaction.into_execution().expect("ABORT set above");
                        proposed_block_change_set.add_transaction_execution(exec)?;
                    }
                }

                // Update the transaction record with any new information provided by this foreign block
                let Some(foreign_evidence) = atom.evidence.get(&foreign_shard_group) else {
                    return Err(ProposalValidationError::ForeignInvalidPledge {
                        block: block.as_leaf_block(),
                        transaction_id: atom.id,
                        shard_group: foreign_shard_group,
                        details: "Foreign proposal did not contain evidence for it's own shard group".to_string(),
                    }
                    .into());
                };
                tx_rec
                    .evidence_mut()
                    .add_shard_group(foreign_shard_group)
                    .update(foreign_evidence)
                    .set_accept_qc(*justify_qc.id());
                tx_rec.set_remote_decision(remote_decision);

                add_pledges(
                    &tx_rec,
                    block.as_leaf_block(),
                    atom,
                    block_pledge,
                    foreign_shard_group,
                    local_committee_info.shard_group(),
                    proposed_block_change_set,
                    false,
                )?;

                // Good debug info
                // tx_rec.evidence().iter().for_each(|(sg, ev)| {
                //     let is_local = local_committee_info.shard_group() == *sg;
                //     log::error!(
                //         target: LOG_TARGET,
                //         "🐞 LOCALACCEPT EVIDENCE (l={}, f={}) {}: {}", is_local, !is_local, sg, ev
                //     );
                // });

                if tx_rec.current_stage().is_new() {
                    // If the transaction is New, we're waiting for all foreign pledges. Propose transaction once we
                    // have them.
                    // CASE: Foreign SGs have pledged all inputs and executed the transaction, local SG is involved
                    // in the outputs
                    let is_ready = //local_committee_info.includes_substate_id(&tx_rec.to_receipt_id().into()) ||
                        tx_rec.committee_involves_inputs(local_committee_info) ||
                        has_all_foreign_input_pledges(tx, &tx_rec, local_committee_info, proposed_block_change_set)?;
                    if is_ready {
                        info!(
                            target: LOG_TARGET,
                            "🧩 FOREIGN PROPOSAL: (Initial sequence from LocalAccept) Transaction is ready for Prepare({}, {}) Local Stage: {}",
                            tx_rec.transaction_id(),
                            tx_rec.current_decision(),
                            tx_rec.current_stage()
                        );

                        tx_rec.set_ready(true);
                        tx_rec.set_next_stage(TransactionPoolStage::New)?;
                        proposed_block_change_set.set_next_transaction_update(tx_rec)?;
                    } else {
                        info!(
                            target: LOG_TARGET,
                            "🧩 FOREIGN PROPOSAL: (Initial sequence from LocalAccept) Transaction is NOT ready for Prepare({}, {}) Local Stage: {}",
                            tx_rec.transaction_id(),
                            tx_rec.current_decision(),
                            tx_rec.current_stage()
                        );

                        // If foreign abort, we are ready to propose ABORT
                        if tx_rec.current_decision().is_abort() {
                            tx_rec.set_ready(true);
                        }
                        proposed_block_change_set.set_next_transaction_update(tx_rec)?;
                    }
                } else if tx_rec.current_stage().is_local_prepared() && tx_rec.is_ready_for_pending_stage() {
                    info!(
                        target: LOG_TARGET,
                        "🧩 FOREIGN PROPOSAL: Transaction is ready for propose ALL_PREPARED({}, {}) Local Stage: {}",
                        tx_rec.transaction_id(),
                        tx_rec.current_decision(),
                        tx_rec.current_stage()
                    );

                    // Set readiness according to the new evidence even if in LocalPrepared phase. We may get
                    // LocalPrepared foreign proposal after this which is basically a no-op
                    tx_rec.set_next_stage(TransactionPoolStage::LocalPrepared)?;
                    proposed_block_change_set.set_next_transaction_update(tx_rec)?;
                } else if tx_rec.current_stage().is_local_accepted() && tx_rec.is_ready_for_pending_stage() {
                    info!(
                        target: LOG_TARGET,
                        "🧩 FOREIGN PROPOSAL: Transaction is ready for propose ALL_ACCEPT({}, {}) Local Stage: {}",
                        tx_rec.transaction_id(),
                        tx_rec.current_decision(),
                        tx_rec.current_stage()
                    );

                    tx_rec.set_next_stage(TransactionPoolStage::LocalAccepted)?;
                    proposed_block_change_set.set_next_transaction_update(tx_rec)?;
                } else {
                    info!(
                        target: LOG_TARGET,
                        "🧩 FOREIGN PROPOSAL: Transaction is NOT ready for ALL_ACCEPT({}, {}) Local Stage: {}, All Justified: {}. Waiting for local proposal.",
                        tx_rec.transaction_id(),
                        tx_rec.current_decision(),
                        tx_rec.current_stage(),
                        tx_rec.evidence().all_shard_groups_accepted()
                    );
                    // Still need to update the evidence
                    proposed_block_change_set.set_next_transaction_update(tx_rec)?;
                }
            },
            // Should never receive this
            Command::EndEpoch => {
                warn!(
                    target: LOG_TARGET,
                    "❓️ NEVER HAPPEN: Foreign proposal received for block {} contains an EndEpoch command. This is invalid behaviour.",
                    block.id()
                );
                continue;
            },
            // TODO(perf): Can we find a way to exclude these unused commands to reduce message size?
            Command::AllAccept(_) |
            Command::SomeAccept(_) |
            Command::AllPrepare(_) |
            Command::SomePrepare(_) |
            Command::Prepare(_) |
            Command::LocalOnly(_) |
            Command::ForeignProposal(_) |
            Command::EvictNode(_) |
            Command::MintConfidentialOutput(_) => {
                // Disregard
                continue;
            },
        }
    }

    info!(
        target: LOG_TARGET,
        "🧩 FOREIGN PROPOSAL: Processed {} commands from foreign block {}",
        command_count,
        block.id()
    );
    if command_count == 0 {
        warn!(
            target: LOG_TARGET,
            "⚠️ FOREIGN PROPOSAL: No commands were applicable for foreign block {}. Ignoring.",
            block.id()
        );
    }

    Ok(())
}

fn add_pledges(
    transaction: &TransactionPoolRecord,
    foreign_block: LeafBlock,
    atom: &TransactionAtom,
    block_pledge: &BlockPledge,
    foreign_shard_group: ShardGroup,
    local_shard_group: ShardGroup,
    proposed_block_change_set: &mut ProposedBlockChangeSet,
    is_prepare_phase: bool,
) -> Result<(), HotStuffError> {
    let _timer = TraceTimer::info(LOG_TARGET, "validate_and_add_pledges");

    // Avoid iterating unless debug logs apply
    if log_enabled!(Level::Debug) {
        debug!(
            target: LOG_TARGET,
            "PLEDGES FOR TRANSACTION: {atom}",
        );
        if block_pledge.is_empty() {
            debug!(
                target: LOG_TARGET,
                "No pledges for transaction {}",
                atom.id
            );
        } else {
            debug!(
                target: LOG_TARGET,
                "FOREIGN PLEDGE {block_pledge}",
            );
        }
    }

    match atom.decision {
        Decision::Commit => {
            // Output pledges come straight from evidence
            let output_pledges = transaction
                .evidence()
                .get(&foreign_shard_group)
                .ok_or_else(|| {
                    // NEVER HAPPEN: we should already have checked this
                    ProposalValidationError::ForeignInvalidPledge {
                        block: foreign_block,
                        transaction_id: atom.id,
                        shard_group: foreign_shard_group,
                        details: "Foreign proposal did not contain evidence for it's own shard group".to_string(),
                    }
                })?
                .to_output_pledge_iter()
                .collect();

            proposed_block_change_set.add_foreign_pledges(
                transaction.transaction_id(),
                foreign_shard_group,
                output_pledges,
            );

            let Some(pledges) = block_pledge.get_transaction_substate_pledges(&atom.id) else {
                if transaction.evidence().is_committee_output_only(foreign_shard_group) {
                    debug!(
                        target: LOG_TARGET,
                        "Foreign proposal for transaction {} stage: {} but the foreign shard group is only involved in outputs so no output pledge is expected.",
                        atom.id,
                        transaction.current_stage()
                    );
                    return Ok(());
                }
                // Accept phase: We only require pledges for inputs in the accept phase if the local shard group is
                // output-only. Otherwise, the pledging has already happened in the prepare
                // phase.
                if !is_prepare_phase && !transaction.evidence().is_committee_output_only(local_shard_group) {
                    debug!(
                        target: LOG_TARGET,
                        "Foreign proposal for transaction {} stage: {}. The local shard group is involved in inputs so pledges should already have been pledged in the prepare phase.",
                        atom.id,
                        transaction.current_stage()
                    );
                    return Ok(());
                }
                warn!(
                    target: LOG_TARGET,
                    "⚠️❌ Foreign proposal for transaction {} stage: {} but no pledges found in the block pledge. Evidence: {}",
                    atom.id,
                    transaction.current_stage(),
                    transaction.evidence()
                );

                return Err(HotStuffError::ForeignNodeOmittedTransactionPledges {
                    foreign_block,
                    transaction_id: atom.id,
                    is_prepare_phase,
                });
            };

            // If the foreign shard has committed the transaction, we can add the pledges to the transaction
            // record
            debug!(
                target: LOG_TARGET,
                "Adding {} foreign pledge(s) to transaction {}. Foreign shard group: {}. Pledges: {}",
                atom.id,
                pledges.len(),
                foreign_shard_group,
                pledges.display()
            );

            proposed_block_change_set.add_foreign_pledges(transaction.transaction_id(), foreign_shard_group, pledges);
        },
        Decision::Abort(reason) => {
            if block_pledge.has_pledges_for(&atom.id) {
                // This is technically a protocol violation but in any case the transaction will be aborted
                warn!(target: LOG_TARGET, "⚠️ Remote decided ABORT({reason}) but provided pledges.");
            } else {
                debug!(target: LOG_TARGET, "Transaction {} was ABORTED by foreign node ({}). No pledges expected.", atom.id, reason);
            }
        },
    }

    Ok(())
}

fn has_all_foreign_input_pledges<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    tx_rec: &TransactionPoolRecord,
    local_committee_info: &CommitteeInfo,
    proposed_block_change_set: &ProposedBlockChangeSet,
) -> Result<bool, HotStuffError> {
    let foreign_inputs = tx_rec
        .evidence()
        .iter()
        .filter(|(sg, _)| local_committee_info.shard_group() != **sg)
        .flat_map(|(_, ev)| ev.inputs());

    let current_pledges = proposed_block_change_set.get_foreign_pledges(tx_rec.transaction_id());

    for (id, data) in foreign_inputs {
        let Some(data) = data else {
            // Case: Foreign shard group evidence is not yet fully populated therefore we do not consider the input
            // pledged
            return Ok(false);
        };
        // Check the current block change set to see if the pledge is included
        if current_pledges
            .clone()
            .any(|pledge| pledge.satisfies_substate_and_version(id, data.version))
        {
            continue;
        }

        if tx.foreign_substate_pledges_exists_for_transaction_and_address(
            tx_rec.transaction_id(),
            SubstateAddress::from_substate_id(id, data.version),
        )? {
            continue;
        }
        debug!(
            target: LOG_TARGET,
            "Transaction {} is missing a pledge for input {}",
            tx_rec.transaction_id(),
            id
        );
        return Ok(false);
    }

    Ok(true)
}
