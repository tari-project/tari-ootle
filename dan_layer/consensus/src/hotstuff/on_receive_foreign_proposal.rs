//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use log::*;
use tari_dan_common_types::{
    committee::CommitteeInfo,
    option::DisplayContainer,
    optional::Optional,
    Epoch,
    ShardGroup,
};
use tari_dan_storage::{
    consensus_models::{
        Block,
        BlockId,
        ForeignProposal,
        ForeignProposalStatus,
        ForeignReceiveCounters,
        QuorumCertificate,
    },
    StateStore,
    StorageError,
};
use tari_epoch_manager::EpochManagerReader;
use tokio::task;

use crate::{
    hotstuff::{error::HotStuffError, pacemaker_handle::PaceMakerHandle, ProposalValidationError},
    messages::{
        ForeignProposalMessage,
        ForeignProposalNotificationMessage,
        ForeignProposalRequestMessage,
        HotstuffMessage,
    },
    tracing::TraceTimer,
    traits::{ConsensusSpec, OutboundMessaging},
};

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::on_receive_foreign_proposal";

#[derive(Clone)]
pub struct OnReceiveForeignProposalHandler<TConsensusSpec: ConsensusSpec> {
    store: TConsensusSpec::StateStore,
    epoch_manager: TConsensusSpec::EpochManager,
    pacemaker: PaceMakerHandle,
    outbound_messaging: TConsensusSpec::OutboundMessaging,
    recently_requested: HashSet<BlockId>,
}

impl<TConsensusSpec> OnReceiveForeignProposalHandler<TConsensusSpec>
where TConsensusSpec: ConsensusSpec
{
    pub fn new(
        store: TConsensusSpec::StateStore,
        epoch_manager: TConsensusSpec::EpochManager,
        pacemaker: PaceMakerHandle,
        outbound_messaging: TConsensusSpec::OutboundMessaging,
    ) -> Self {
        Self {
            store,
            epoch_manager,
            pacemaker,
            outbound_messaging,
            recently_requested: HashSet::new(),
        }
    }

    pub async fn handle_received(
        &mut self,
        message: ForeignProposalMessage,
        local_committee_info: &CommitteeInfo,
    ) -> Result<(), HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "OnReceiveForeignProposal");
        let proposal = ForeignProposal::from(message);

        if self.store.with_read_tx(|tx| proposal.exists(tx))? {
            // This is expected behaviour, we may receive the same foreign proposal multiple times
            debug!(
                target: LOG_TARGET,
                "FOREIGN PROPOSAL: Already received proposal for block {}",
                proposal.block().id(),
            );
            self.remove_recently_requested(proposal.block().id());
            return Ok(());
        }

        let foreign_committee_info = self
            .epoch_manager
            .get_committee_info_by_validator_public_key(proposal.block.epoch(), proposal.block.proposed_by().clone())
            .await?;
        let block_id = *proposal.block().id();
        self.store.with_write_tx(|tx| {
            if let Err(err) = self.validate_and_save(tx, proposal, local_committee_info, &foreign_committee_info) {
                error!(target: LOG_TARGET, "❌ Error validating and saving foreign proposal: {}", err);
                // Should not cause consensus to crash and should still be committed (Invalid proposal status)
                return Ok(());
            }
            Ok::<_, HotStuffError>(())
        })?;

        // TODO: keep track of requested proposals and if there are any non-responses after a certain time, request from
        // another node
        self.remove_recently_requested(&block_id);

        // Foreign proposals to propose
        self.pacemaker.beat();
        Ok(())
    }

    fn remove_recently_requested(&mut self, block_id: &BlockId) {
        self.recently_requested.remove(block_id);
        if self.recently_requested.capacity() - self.recently_requested.len() > 50 {
            self.recently_requested.shrink_to_fit();
        }
    }

    pub async fn handle_notification_received(
        &mut self,
        from: TConsensusSpec::Addr,
        current_epoch: Epoch,
        message: ForeignProposalNotificationMessage,
        local_committee_info: &CommitteeInfo,
    ) -> Result<(), HotStuffError> {
        debug!(
            target: LOG_TARGET,
            "🌐 Receive FOREIGN PROPOSAL NOTIFICATION from {} for block {}",
            from,
            message.block_id,
        );
        if self.recently_requested.contains(&message.block_id) {
            warn!(
                target: LOG_TARGET,
                "🌐 FOREIGN PROPOSAL: Already requested block {}. Ignoring.",
                message.block_id,
            );
            return Ok(());
        }
        if self
            .store
            .with_read_tx(|tx| ForeignProposal::record_exists(tx, &message.block_id))?
        {
            // This is expected behaviour, we may receive the same foreign proposal notification multiple times
            debug!(
                target: LOG_TARGET,
                "FOREIGN PROPOSAL: Already received proposal for block {}",
                message.block_id,
            );
            return Ok(());
        }

        // Check if the source is in a foreign committee
        let foreign_committee_info = self
            .epoch_manager
            .get_committee_info_by_validator_address(message.epoch, &from)
            .await?;

        if local_committee_info.shard_group() == foreign_committee_info.shard_group() {
            warn!(
                target: LOG_TARGET,
                "❓️ FOREIGN PROPOSAL: Received foreign proposal notification from a validator in the same shard group. Ignoring."
            );
            return Ok(());
        }

        let f = local_committee_info.max_failures() as usize;
        let committee = self
            .epoch_manager
            .get_committee_by_shard_group(current_epoch, foreign_committee_info.shard_group(), Some(f + 1))
            .await?;

        let Some((selected, _)) = committee.shuffled().next() else {
            warn!(
                target: LOG_TARGET,
                "FOREIGN PROPOSAL: No validator selected for the shard group {}",
                foreign_committee_info.shard_group(),
            );
            return Ok(());
        };

        info!(
            target: LOG_TARGET,
            "🌐 REQUEST foreign proposal for block {} from {}",
            message.block_id,
            selected,
        );
        self.outbound_messaging
            .send(
                selected.clone(),
                HotstuffMessage::ForeignProposalRequest(ForeignProposalRequestMessage::ByBlockId {
                    block_id: message.block_id,
                    for_shard_group: local_committee_info.shard_group(),
                    epoch: message.epoch,
                }),
            )
            .await?;

        self.recently_requested.insert(message.block_id);

        Ok(())
    }

    pub async fn handle_requested(
        &mut self,
        from: TConsensusSpec::Addr,
        message: ForeignProposalRequestMessage,
        local_committee_info: &CommitteeInfo,
    ) -> Result<(), HotStuffError> {
        let store = self.store.clone();
        let outbound_messaging = self.outbound_messaging.clone();
        let local_committee_info = *local_committee_info;

        // No need for consensus to wait for the task to complete
        task::spawn(async move {
            let _timer = TraceTimer::debug(LOG_TARGET, "OnReceiveForeignProposalRequest");
            if let Err(err) =
                Self::handle_requested_task(store, outbound_messaging, from, message, &local_committee_info).await
            {
                error!(target: LOG_TARGET, "Error handling requested foreign proposal: {}", err);
            }
        });

        Ok(())
    }

    async fn handle_requested_task(
        store: TConsensusSpec::StateStore,
        mut outbound_messaging: TConsensusSpec::OutboundMessaging,
        from: TConsensusSpec::Addr,
        message: ForeignProposalRequestMessage,
        local_committee_info: &CommitteeInfo,
    ) -> Result<(), HotStuffError> {
        match message {
            ForeignProposalRequestMessage::ByBlockId {
                block_id,
                for_shard_group,
                ..
            } => {
                info!(
                    target: LOG_TARGET,
                    "🌐 HANDLE foreign proposal request from {} for {}",
                    for_shard_group,
                    block_id,
                );
                let Some((block, justify_qc, mut block_pledge)) = store
                    .with_read_tx(|tx| {
                        let block = Block::get(tx, &block_id)?;
                        let justify_qc = QuorumCertificate::get_by_block_id(tx, &block_id)?;
                        let block_pledge = block.get_block_pledge(tx)?;
                        Ok::<_, StorageError>((block, justify_qc, block_pledge))
                    })
                    .optional()?
                else {
                    warn!(
                        target: LOG_TARGET,
                        "FOREIGN PROPOSAL: Requested block {} not found. Ignoring.",
                        block_id,
                    );
                    return Ok(());
                };

                let applicable_transactions = block
                    .commands()
                    .iter()
                    .filter_map(|c| {
                        c.local_prepare()
                            // No need to broadcast LocalPrepare if the committee is output only
                            .filter(|atom| !atom.evidence.is_committee_output_only(local_committee_info.shard_group()))
                            .or_else(|| c.local_accept())
                    })
                    .filter(|atom| atom.evidence.has(&for_shard_group))
                    .map(|atom| atom.id)
                    .collect::<HashSet<_>>();

                debug!(
                    target: LOG_TARGET,
                    "🌐 FOREIGN PROPOSAL: Requested block {} contains {} applicable transaction(s) for {} ({} LocalPrepare*/LocalAccept tx(s) in block)",
                    block_id,
                    applicable_transactions.len(),
                    for_shard_group,
                    block_pledge.len(),
                );

                if applicable_transactions.is_empty() {
                    warn!(
                        target: LOG_TARGET,
                        "⚠️ FOREIGN PROPOSAL: Requested block {} does not contain any applicable transactions for {}. Ignoring.",
                        block_id,
                        for_shard_group,
                    );
                    return Ok(());
                }

                // Only send the pledges for the involved shard group that requested them
                block_pledge.retain_transactions(&applicable_transactions);

                info!(
                    target: LOG_TARGET,
                    "🌐 REPLY foreign proposal {} {} pledge(s) ({} tx(s)) to {}. justify: {} ({}), parent: {}",
                    block.as_leaf_block(),
                    block_pledge.num_substates_pledged(),
                    block_pledge.len(),
                    for_shard_group,
                    justify_qc.block_id(),
                    justify_qc.block_height(),
                    block.parent()
                );

                outbound_messaging
                    .send(
                        from,
                        HotstuffMessage::ForeignProposal(ForeignProposalMessage {
                            block,
                            justify_qc,
                            block_pledge,
                        }),
                    )
                    .await?;
            },
            ForeignProposalRequestMessage::ByTransactionId { .. } => {
                error!(
                    target: LOG_TARGET,
                    "TODO FOREIGN PROPOSAL: Request by transaction id is not supported. Ignoring."
                );
            },
        }

        Ok(())
    }

    pub fn validate_and_save(
        &self,
        tx: &mut <TConsensusSpec::StateStore as StateStore>::WriteTransaction<'_>,
        proposal: ForeignProposal,
        local_committee_info: &CommitteeInfo,
        foreign_committee_info: &CommitteeInfo,
    ) -> Result<(), HotStuffError> {
        let mut foreign_receive_counter = ForeignReceiveCounters::get_or_default(&**tx)?;

        if let Err(err) = self.validate_proposed_block(
            &proposal,
            foreign_committee_info.shard_group(),
            local_committee_info.shard_group(),
            &foreign_receive_counter,
        ) {
            // TODO: handle this case. Perhaps, by aborting all transactions that are affected by this block (we known
            // the justify QC is valid)
            warn!(
                target: LOG_TARGET,
                "⚠️ FOREIGN PROPOSAL: Invalid proposal: {}. Ignoring {}.",
                err,
                proposal.block(),
            );
            proposal.upsert(tx, None)?;
            proposal.set_status(tx, ForeignProposalStatus::Invalid)?;
            return Err(err.into());
        }

        foreign_receive_counter.increment_group(foreign_committee_info.shard_group());

        info!(
            target: LOG_TARGET,
            "🧩 Receive FOREIGN PROPOSAL {}, justify_qc: {}",
            proposal.block(),
            proposal.justify_qc(),
        );

        foreign_receive_counter.save(tx)?;
        proposal.upsert(tx, None)?;

        Ok(())
    }

    fn validate_proposed_block(
        &self,
        proposal: &ForeignProposal,
        foreign_shard: ShardGroup,
        local_shard: ShardGroup,
        _foreign_receive_counter: &ForeignReceiveCounters,
    ) -> Result<(), ProposalValidationError> {
        // TODO: validations specific to the foreign proposal. General block validations (signature etc) are already
        //       performed in on_message_validate.

        if proposal.justify_qc().block_id() != proposal.block().id() {
            warn!(
                target: LOG_TARGET,
                "⚠️ FOREIGN PROPOSAL: Justify QC block id does not match the block id. Justify QC block id: {}, block id: {}",
                proposal.justify_qc().block_id(),
                proposal.block().id(),
            );
            return Err(ProposalValidationError::ForeignJustifyQcDoesNotJustifyProposal {
                block_id: *proposal.block().id(),
                justify_qc_block_id: *proposal.justify_qc().block_id(),
                shard_group: foreign_shard,
            });
        }

        validate_evidence_and_pledges_match(proposal, local_shard, foreign_shard)?;

        // TODO: ignoring for now because this is currently broken
        // let Some(incoming_count) = candidate_block.get_foreign_counter(&local_shard) else {
        //     debug!(target:LOG_TARGET, "Our bucket {local_shard:?} is missing reliability index in the proposed block
        // {candidate_block:?}");     return Err(ProposalValidationError::MissingForeignCounters {
        //         proposed_by: from.to_string(),
        //         hash: *candidate_block.id(),
        //     });
        // };
        // let current_count = foreign_receive_counter.get_count(&foreign_shard);
        // if current_count + 1 != incoming_count {
        //     debug!(target:LOG_TARGET, "We were expecting the index to be {expected_count}, but the index was
        // {incoming_count}", expected_count = current_count + 1);     return
        // Err(ProposalValidationError::InvalidForeignCounters {         proposed_by: from.to_string(),
        //         hash: *candidate_block.id(),
        //         details: format!(
        //             "Expected foreign receive count to be {} but it was {}",
        //             current_count + 1,
        //             incoming_count
        //         ),
        //     });
        // }

        Ok(())
    }
}

fn validate_evidence_and_pledges_match(
    proposal: &ForeignProposal,
    local_shard: ShardGroup,
    foreign_shard: ShardGroup,
) -> Result<(), ProposalValidationError> {
    // TODO: any error will** result in transactions that never resolve.
    // ** unless the foreign shard sends it again with the correct evidence and pledges
    // Possible ways to handle this:
    // - Send a message to the foreign shard to request the pledges again (but why would they return the correct pledges
    //   this time?)
    // - Immediately ABORT all transactions with invalid pledges in the block - this is the safest option
    let mut num_applicable = 0usize;
    for atom in proposal.block().commands().iter().filter_map(|cmd| {
        cmd.local_prepare()
            // The foreign committee may have sent us this block for other transactions that are applicable to us
            // not for this output-only LocalPrepare
            .filter(|atom| atom.evidence.is_committee_output_only(foreign_shard))
            .or_else(|| cmd.local_accept())
    }) {
        if atom.decision.is_abort() || !atom.evidence.has(&local_shard) {
            continue;
        }

        num_applicable += 1;

        let pledges = proposal.block_pledge.get_transaction_pledges(&atom.id).ok_or_else(|| {
            ProposalValidationError::ForeignInvalidPledge {
                transaction_id: atom.id,
                block_id: *proposal.block().id(),
                shard_group: foreign_shard,
                details: "substate pledges for transaction are missing".to_string(),
            }
        })?;

        let evidence =
            atom.evidence
                .get(&foreign_shard)
                .ok_or_else(|| ProposalValidationError::ForeignInvalidPledge {
                    transaction_id: atom.id,
                    block_id: *proposal.block().id(),
                    shard_group: foreign_shard,
                    details: "evidence for transaction is missing".to_string(),
                })?;

        for (input, _) in evidence.inputs() {
            if pledges.iter().all(|p| p.substate_id() != input) {
                warn!(
                    target: LOG_TARGET,
                    "⚠️ FOREIGN PROPOSAL: Invalid proposal: substate pledge for input {} is missing. Pledges: {}",
                    input,
                    pledges.display(),
                );
                return Err(ProposalValidationError::ForeignInvalidPledge {
                    transaction_id: atom.id,
                    block_id: *proposal.block().id(),
                    shard_group: foreign_shard,
                    details: format!("substate pledge for input {input} is missing"),
                });
            }
        }
    }

    info!(
        target: LOG_TARGET,
        "🧩 FOREIGN PROPOSAL: OK - {} of {} command(s) apply, {} transaction pledge(s) in {}",
        num_applicable,
        proposal.block().commands().len(),
        proposal.block_pledge().len(),
        proposal.block(),
    );

    Ok(())
}
