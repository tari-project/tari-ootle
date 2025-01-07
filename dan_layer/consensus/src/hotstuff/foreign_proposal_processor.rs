//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_dan_common_types::{committee::CommitteeInfo, option::DisplayContainer, SubstateAddress, ToSubstateAddress};
use tari_dan_storage::{
    consensus_models::{
        BlockId,
        BlockPledge,
        Command,
        Decision,
        ForeignProposal,
        LeafBlock,
        LockedBlock,
        TransactionAtom,
        TransactionPoolRecord,
        TransactionPoolStage,
        TransactionRecord,
    },
    StateStoreReadTransaction,
};
use tari_engine_types::commit_result::RejectReason;

use crate::{
    hotstuff::{block_change_set::ProposedBlockChangeSet, error::HotStuffError, ProposalValidationError},
    tracing::TraceTimer,
};

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::foreign_proposal_processor";

#[allow(clippy::too_many_lines)]
pub fn process_foreign_block<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    local_leaf: &LeafBlock,
    locked_block: &LockedBlock,
    proposal: ForeignProposal,
    foreign_committee_info: &CommitteeInfo,
    local_committee_info: &CommitteeInfo,
    proposed_block_change_set: &mut ProposedBlockChangeSet,
) -> Result<(), HotStuffError> {
    let _timer = TraceTimer::info(LOG_TARGET, "process_foreign_block");
    assert_eq!(
        proposal.block.shard_group(),
        foreign_committee_info.shard_group(),
        "Foreign proposal shard group does not match the foreign committee shard group"
    );
    info!(
        target: LOG_TARGET,
        "🧩 Processing FOREIGN PROPOSAL {}, justify_qc: {}",
        proposal.block(),
        proposal.justify_qc(),
    );

    let ForeignProposal {
        block,
        justify_qc,
        mut block_pledge,
        ..
    } = proposal;
    let mut command_count = 0usize;

    for cmd in block.commands() {
        match cmd {
            Command::LocalPrepare(atom) => {
                if !atom.evidence.has(&local_committee_info.shard_group()) {
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

                let Some(mut tx_rec) =
                    proposed_block_change_set.get_transaction(tx, locked_block, local_leaf, &atom.id)?
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
                        // May be a bug in the foreign missing transaction handling
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
                        transaction.set_abort_reason(RejectReason::ForeignShardGroupDecidedToAbort {
                            start_shard: foreign_committee_info.shard_group().start().as_u32(),
                            end_shard: foreign_committee_info.shard_group().end().as_u32(),
                            abort_reason: abort_reason.to_string(),
                        });
                        let exec = transaction.into_execution().expect("ABORT set above");
                        proposed_block_change_set.add_transaction_execution(exec)?;
                    }
                }

                // Update the transaction record with any new information provided by this foreign block
                let Some(foreign_evidence) = atom.evidence.get(&foreign_committee_info.shard_group()) else {
                    return Err(ProposalValidationError::ForeignInvalidPledge {
                        block_id: *block.id(),
                        transaction_id: atom.id,
                        shard_group: foreign_committee_info.shard_group(),
                        details: "Foreign proposal did not contain evidence for shard group".to_string(),
                    }
                    .into());
                };
                tx_rec
                    .evidence_mut()
                    .add_shard_group(foreign_committee_info.shard_group())
                    .update(foreign_evidence)
                    .set_prepare_qc(*justify_qc.id());
                tx_rec.set_remote_decision(remote_decision);

                validate_and_add_pledges(
                    &tx_rec,
                    block.id(),
                    atom,
                    &mut block_pledge,
                    foreign_committee_info,
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
                    let is_ready = local_committee_info.includes_substate_id(&tx_rec.to_receipt_id().into()) ||
                        tx_rec.involves_committee(local_committee_info) ||
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
                    // If all shards are complete, and we've already received our LocalPrepared, we can set out
                    // LocalPrepared transaction as ready to propose ACCEPT. If we have not received
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

                let Some(mut tx_rec) =
                    proposed_block_change_set.get_transaction(tx, locked_block, local_leaf, &atom.id)?
                else {
                    warn!(
                        target: LOG_TARGET,
                        "⚠️ NEVER HAPPEN: Foreign proposal {} received for transaction {} but this transaction is not in the pool.",
                        block.id(),
                        atom.id
                    );
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
                        transaction.set_abort_reason(RejectReason::ForeignShardGroupDecidedToAbort {
                            start_shard: foreign_committee_info.shard_group().start().as_u32(),
                            end_shard: foreign_committee_info.shard_group().end().as_u32(),
                            abort_reason: abort_reason.to_string(),
                        });
                        let exec = transaction.into_execution().expect("ABORT set above");
                        proposed_block_change_set.add_transaction_execution(exec)?;
                    }
                }

                // Update the transaction record with any new information provided by this foreign block
                let Some(foreign_evidence) = atom.evidence.get(&foreign_committee_info.shard_group()) else {
                    return Err(ProposalValidationError::ForeignInvalidPledge {
                        block_id: *block.id(),
                        transaction_id: atom.id,
                        shard_group: foreign_committee_info.shard_group(),
                        details: "Foreign proposal did not contain evidence for shard group".to_string(),
                    }
                    .into());
                };
                tx_rec
                    .evidence_mut()
                    .add_shard_group(foreign_committee_info.shard_group())
                    .update(foreign_evidence)
                    .set_accept_qc(*justify_qc.id());
                tx_rec.set_remote_decision(remote_decision);

                validate_and_add_pledges(
                    &tx_rec,
                    block.id(),
                    atom,
                    &mut block_pledge,
                    foreign_committee_info,
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
                    let is_ready = local_committee_info.includes_substate_id(&tx_rec.to_receipt_id().into()) ||
                        tx_rec.involves_committee(local_committee_info) ||
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
                        tx_rec.evidence().all_objects_accepted()
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

#[allow(clippy::too_many_lines)]
fn validate_and_add_pledges(
    transaction: &TransactionPoolRecord,
    foreign_block_id: &BlockId,
    atom: &TransactionAtom,
    block_pledge: &mut BlockPledge,
    foreign_committee_info: &CommitteeInfo,
    proposed_block_change_set: &mut ProposedBlockChangeSet,
    is_prepare_phase: bool,
) -> Result<(), HotStuffError> {
    let _timer = TraceTimer::info(LOG_TARGET, "validate_and_add_pledges");
    // We need to add the justify QC to the evidence because the prepare block should not include it
    // yet
    let evidence = atom
        .evidence
        .get(&foreign_committee_info.shard_group())
        .ok_or_else(|| ProposalValidationError::ForeignInvalidPledge {
            block_id: *foreign_block_id,
            transaction_id: atom.id,
            shard_group: foreign_committee_info.shard_group(),
            details: format!(
                "Foreign proposal did not contain evidence for {}",
                foreign_committee_info.shard_group()
            ),
        })?;

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
            let Some(pledges) = block_pledge.remove_transaction_pledges(&atom.id) else {
                if transaction.is_global() {
                    // If the transaction is global, some shard groups do not pledges to include
                    // TODO: this is currently "assumed" to be correct and should be validated
                    debug!(
                        target: LOG_TARGET,
                        "Foreign proposal COMMIT for transaction {} stage: {} but the transaction is global so no pledges are expected.",
                        atom.id,
                        transaction.current_stage()
                    );
                    return Ok(());
                }

                if is_prepare_phase &&
                    atom.evidence
                        .is_committee_output_only(foreign_committee_info.shard_group())
                {
                    // If the foreign shard group is only involved in the outputs, there will not be any pledges in the
                    // prepare phase
                    debug!(
                        target: LOG_TARGET,
                        "Foreign proposal COMMIT for transaction {} stage: {} but the foreign shard group is only involved in outputs so no output pledge is expected.",
                        atom.id,
                        transaction.current_stage()
                    );
                    return Ok(());
                }
                return Err(HotStuffError::ForeignNodeOmittedTransactionPledges {
                    foreign_block_id: *foreign_block_id,
                    transaction_id: atom.id,
                });
            };

            // Validate that provided evidence is correct
            // TODO: there are a lot of validations to be done on evidence and the foreign block in general,
            // this is here as a sanity check
            for pledge in &pledges {
                if pledge.is_input() {
                    if !evidence.inputs().contains_key(pledge.substate_id()) {
                        let address = pledge.versioned_substate_id().to_substate_address();
                        return Err(ProposalValidationError::ForeignInvalidPledge {
                            block_id: *foreign_block_id,
                            transaction_id: atom.id,
                            shard_group: foreign_committee_info.shard_group(),
                            details: format!("Pledge {pledge} for address {address} not found in input evidence"),
                        }
                        .into());
                    }
                } else if !evidence.outputs().contains_key(pledge.substate_id()) {
                    let address = pledge.versioned_substate_id().to_substate_address();
                    return Err(ProposalValidationError::ForeignInvalidPledge {
                        block_id: *foreign_block_id,
                        transaction_id: atom.id,
                        shard_group: foreign_committee_info.shard_group(),
                        details: format!("Pledge {pledge} for address {address} not found in output evidence"),
                    }
                    .into());
                } else {
                    debug!(
                        target: LOG_TARGET,
                        "Foreign pledge {} for transaction {} found in evidence",
                        pledge,
                        atom.id
                    );
                    // Ok
                }
            }

            // If the foreign shard has committed the transaction, we can add the pledges to the transaction
            // record
            debug!(
                target: LOG_TARGET,
                "Adding foreign pledges to transaction {}. Foreign shard group: {}. Pledges: {}",
                atom.id,
                foreign_committee_info.shard_group(),
                pledges.display()
            );
            proposed_block_change_set.add_foreign_pledges(
                transaction.transaction_id(),
                foreign_committee_info.shard_group(),
                pledges,
            );
        },
        Decision::Abort(reason) => {
            warn!(target: LOG_TARGET, "⚠️ Remote decided ABORT({reason:?}) but provided pledges.");
            if block_pledge.contains(&atom.id) {
                return Err(ProposalValidationError::ForeignInvalidPledge {
                    block_id: *foreign_block_id,
                    transaction_id: atom.id,
                    shard_group: foreign_committee_info.shard_group(),
                    details: "Remote decided ABORT but provided pledges".to_string(),
                }
                .into());
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

        if tx.foreign_substate_pledges_exists_for_address(
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
