//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use anyhow::anyhow;
use log::*;
use tari_consensus_types::{BlockId, ProposalCertificate};
use tari_dan_common_types::{committee::CommitteeInfo, optional::Optional, Epoch, ShardGroup};
use tari_dan_storage::{
    consensus_models::{
        Block,
        CommandOrHash,
        CommandsCommitProof,
        ForeignProposal,
        ForeignProposalRecord,
        ForeignProposalStatus,
    },
    StateStore,
    StateStoreReadTransaction,
};
use tari_epoch_manager::EpochManagerReader;
use tokio::task;

use crate::{
    hotstuff::{
        commit_proofs::generate_block_commit_proof,
        error::HotStuffError,
        pacemaker_handle::PaceMakerHandle,
        ProposalValidationError,
    },
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
        let mut proposal = ForeignProposalRecord::from(message);

        let block_id = *proposal.block_id();
        if self.store.with_read_tx(|tx| proposal.exists(tx))? {
            // This is expected behaviour, we may receive the same foreign proposal multiple times
            debug!(
                target: LOG_TARGET,
                "FOREIGN PROPOSAL: Already received proposal for block {}",
                block_id
            );
            self.remove_recently_requested(&block_id);
            return Ok(());
        }

        self.store.with_write_tx(|tx| {
            if let Err(err) = self.validate_and_save(tx, &proposal, local_committee_info) {
                error!(target: LOG_TARGET, "❌ Error validating and saving foreign proposal: {}", err);
                // Should not cause consensus to crash and should commit the Invalid proposal status
                proposal.save(tx)?;
                proposal.set_status(tx, ForeignProposalStatus::Invalid, None)?;
                // TODO: reattempt from different node? and then abort on persistent failure
                // If we miss a foreign proposal, we want to implement the ability to request it - so we could just rely
                // on that functionality without doing anything extra here
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
        if self.recently_requested.capacity() - self.recently_requested.len() > 1000 {
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
            info!(
                target: LOG_TARGET,
                "🌐 FOREIGN PROPOSAL: Already requested block {}. Ignoring.",
                message.block_id,
            );
            return Ok(());
        }
        if self
            .store
            .with_read_tx(|tx| ForeignProposalRecord::record_exists(tx, &message.block_id))?
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
    ) -> Result<(), HotStuffError> {
        let store = self.store.clone();
        let outbound_messaging = self.outbound_messaging.clone();

        // No need for consensus to wait for the task to complete
        task::spawn(async move {
            let _timer = TraceTimer::debug(LOG_TARGET, "OnReceiveForeignProposalRequest");
            if let Err(err) = Self::handle_requested_task(store, outbound_messaging, from, message).await {
                error!(target: LOG_TARGET, "Error handling requested foreign proposal: {}", err);
            }
        });

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn handle_requested_task(
        store: TConsensusSpec::StateStore,
        mut outbound_messaging: TConsensusSpec::OutboundMessaging,
        from: TConsensusSpec::Addr,
        message: ForeignProposalRequestMessage,
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
                let Some(proposal) = store.with_read_tx(|tx| {
                    let Some(block) = Block::get(tx, &block_id).optional()? else {
                        return Ok(None);
                    };
                    let commit_qc = block.get_commit_qc(tx)?;
                    let block_pledge = block.get_block_pledge(tx, for_shard_group)?;

                    let commit_proof = generate_transaction_commands_commit_proof_for_shard_group(
                        tx,
                        &block,
                        &commit_qc,
                        for_shard_group,
                    )?;
                    let proposal = ForeignProposal::new(commit_proof, block_pledge);
                    Ok::<_, HotStuffError>(Some(proposal))
                })?
                else {
                    warn!(
                        target: LOG_TARGET,
                        "FOREIGN PROPOSAL: Requested block {} not found. Ignoring.",
                        block_id,
                    );
                    return Ok(());
                };

                info!(
                    target: LOG_TARGET,
                    "🌐 FOREIGN PROPOSAL REPLY to {} foreign proposal {} with {} pledge(s).",
                    for_shard_group,
                    proposal.calculate_block_id(),
                    proposal.block_pledge().len(),
                );

                outbound_messaging
                    .send(from, HotstuffMessage::ForeignProposal(proposal.into()))
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
        proposal: &ForeignProposalRecord,
        local_committee_info: &CommitteeInfo,
    ) -> Result<(), HotStuffError> {
        if let Err(err) = self.validate_foreign_proposal(proposal.proposal(), local_committee_info) {
            // TODO: handle this case. Perhaps, by aborting all transactions that are affected by this block (we known
            // the justify QC is valid)
            warn!(
                target: LOG_TARGET,
                "⚠️❌ FOREIGN PROPOSAL: Invalid proposal: {}. Ignoring {}.",
                err,
                proposal,
            );
            return Err(err.into());
        }

        info!(
            target: LOG_TARGET,
            "🧩 Receive FOREIGN PROPOSAL {}",
            proposal
        );

        proposal.save(tx)?;

        Ok(())
    }

    fn validate_foreign_proposal(
        &self,
        proposal: &ForeignProposal,
        local_committee_info: &CommitteeInfo,
    ) -> Result<(), ProposalValidationError> {
        // TODO: validations specific to the foreign proposal. General block validations (signature etc) are already
        //       performed in on_message_validate. These should be in the validator helper module.

        if proposal.epoch() != local_committee_info.epoch() &&
            // Allow one epoch behind as Prepare/Accept rounds may have been conducted in the previous epoch before epoch end
            proposal.epoch() != local_committee_info.epoch() - Epoch(1)
        {
            warn!(
                target: LOG_TARGET,
                "⚠️ FOREIGN PROPOSAL: Invalid proposal epoch: {}. Current epoch: {}",
                proposal.epoch(),
                local_committee_info.epoch(),
            );
            return Err(ProposalValidationError::ForeignProposalInvalid {
                block_id: proposal.calculate_block_id(),
                shard_group: proposal.shard_group_unchecked(),
                details: anyhow!(
                    "Foreign node proposal epoch is not within range of the current epoch. Current epoch: {}, block \
                     epoch: {}",
                    local_committee_info.epoch(),
                    proposal.epoch(),
                ),
            });
        }

        validate_evidence_and_pledges_match(proposal, local_committee_info.shard_group())?;

        Ok(())
    }
}

fn validate_evidence_and_pledges_match(
    proposal: &ForeignProposal,
    local_shard_group: ShardGroup,
) -> Result<(), ProposalValidationError> {
    let foreign_shard_group = proposal.shard_group_checked().ok_or_else(|| {
            warn!(
                target: LOG_TARGET,
                "⚠️ FOREIGN PROPOSAL: Invalid proposal: foreign shard group does not match the local shard group. Local Shard Group: {}, Foreign Shard Group: {}",
                local_shard_group,
                proposal.shard_group_unchecked(),
            );
            ProposalValidationError::ForeignProposalInvalid {
                block_id: proposal.calculate_block_id(),
                shard_group: proposal.shard_group_unchecked(),
                details: anyhow!("Invalid shard group bounds")
            }
        })?;
    // TODO: any error will** result in transactions that never resolve.
    // ** unless the foreign shard sends it again with the correct evidence and pledges
    // Possible ways to handle this:
    // - Send a message to the foreign shard to request the pledges again (but why would they return the correct pledges
    //   this time?)
    // - Immediately ABORT all transactions with invalid pledges in the block - this is the safest option
    let mut num_applicable = 0usize;
    for (is_local_accept, atom) in proposal.full_commands_iter().filter_map(|cmd| {
        cmd.local_prepare()
            // The foreign committee may have sent us this block for other transactions that are applicable to us
            // not for this output-only LocalPrepare
            .filter(|atom| !atom.evidence.is_committee_output_only(proposal.shard_group_unchecked()))
            .map(|atom| (false, atom))
            .or_else(|| cmd.local_accept().map(|atom| (true, atom)))
    }) {
        if atom.decision.is_abort() || !atom.evidence.has(&local_shard_group) {
            continue;
        }

        num_applicable += 1;

        // If the local node is involved in inputs (i.e not output-only), we already have the input pledges from the
        // prepare phase and so do not need them to be resent.
        let dont_need_input_pledges = is_local_accept &&
            (!atom.evidence.is_committee_output_only(local_shard_group) ||
                atom.evidence.is_committee_output_only(foreign_shard_group));
        if dont_need_input_pledges {
            // CASE: if we're an input shard group, and we're receiving a local accept, we do not require input pledges
            debug!(
                target: LOG_TARGET,
                "FOREIGN PROPOSAL: Input pledges not required for LocalAccept"
            );
            continue;
        }

        debug!(
            target: LOG_TARGET,
            "🧩 FOREIGN PROPOSAL: Check transaction {} pledges from {} - local_shard_group: {}, is_local_accept: {}, local-output-only: {}",
            atom.id,
            foreign_shard_group,
            local_shard_group,
            is_local_accept,
            atom.evidence.is_committee_output_only(local_shard_group),
        );

        let shard_group_evidence =
            atom.evidence
                .get(&foreign_shard_group)
                .ok_or_else(|| ProposalValidationError::ForeignProposalInvalid {
                    block_id: proposal.calculate_block_id(),
                    shard_group: proposal.shard_group_unchecked(),
                    details: anyhow!(
                        "InvalidPledge: Atom({}) evidence from foreign shard group does not contain evidence for that \
                         shard group",
                        atom.id
                    ),
                })?;

        if !proposal
            .block_pledge()
            .has_all_input_substate_values_for(shard_group_evidence)
        {
            warn!(
                target: LOG_TARGET,
                "⚠️ FOREIGN PROPOSAL: Invalid proposal: some input pledges for {}({}) are missing. Local Shard Group: {}, All Pledges: {}",
                if is_local_accept { "LocalAccept" } else { "LocalPrepare" },
                atom.id,
                local_shard_group,
                proposal.block_pledge(),
            );
            return Err(ProposalValidationError::ForeignProposalInvalid {
                block_id: proposal.calculate_block_id(),
                shard_group: foreign_shard_group,
                details: anyhow!(
                    "input pledges are missing one or more substate values for transaction {}",
                    atom.id
                ),
            });
        }
    }

    info!(
        target: LOG_TARGET,
        "🧩 FOREIGN PROPOSAL: OK - {} of {} command(s) apply in {}",
        num_applicable,
        proposal.full_commands_iter().count(),
        proposal,
    );

    Ok(())
}

fn generate_transaction_commands_commit_proof_for_shard_group<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    block: &Block,
    commit_qc: &ProposalCertificate,
    for_shard_group: ShardGroup,
) -> Result<CommandsCommitProof, HotStuffError> {
    let applicable_commands = block.commands().iter().map(|cmd| {
        let is_involved_local_prepare_with_inputs = cmd
            .local_prepare()
            .map(|atom| atom.evidence.has_inputs(for_shard_group))
            .unwrap_or(false);

        let is_involved_local_accept = cmd
            .local_accept()
            .map(|atom| atom.evidence.has(&for_shard_group))
            .unwrap_or(false);

        if is_involved_local_prepare_with_inputs || is_involved_local_accept {
            CommandOrHash::Command(cmd.clone())
        } else {
            CommandOrHash::Hash(cmd.hash())
        }
    });

    let proof = generate_block_commit_proof(tx, commit_qc, block)?;
    let command_commit_proof = CommandsCommitProof::new_latest(applicable_commands.collect(), proof);
    Ok(command_commit_proof)
}
