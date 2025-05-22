//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_consensus_types::{
    HighPc,
    LastSentVote,
    LeafBlock,
    ProposalCertificate,
    TimeoutVote,
    TimeoutVoteMessage,
    ValidatorSignatureBytes,
};
use tari_dan_common_types::{committee::Committee, displayable::Displayable, optional::Optional, Epoch, NodeHeight};
use tari_dan_storage::{consensus_models::BookkeepingModel, StateStore};
use tari_engine_types::ToByteType;

use crate::{
    hotstuff::{get_leader_for_view, HotStuffError},
    messages::{HotstuffMessage, NewViewMessage},
    traits::{CertificateStore, ConsensusSpec, OutboundMessaging, ValidatorSignerService},
};

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::on_next_sync_view";

pub struct OnNextSyncViewHandler<TConsensusSpec: ConsensusSpec> {
    store: TConsensusSpec::StateStore,
    outbound_messaging: TConsensusSpec::OutboundMessaging,
    leader_strategy: TConsensusSpec::LeaderStrategy,
    signer_service: TConsensusSpec::SignerService,
}

impl<TConsensusSpec: ConsensusSpec> OnNextSyncViewHandler<TConsensusSpec> {
    pub fn new(
        store: TConsensusSpec::StateStore,
        outbound_messaging: TConsensusSpec::OutboundMessaging,
        leader_strategy: TConsensusSpec::LeaderStrategy,
        signer_service: TConsensusSpec::SignerService,
    ) -> Self {
        Self {
            store,
            outbound_messaging,
            leader_strategy,
            signer_service,
        }
    }

    pub async fn handle(
        &mut self,
        epoch: Epoch,
        current_height: NodeHeight,
        local_committee: &Committee<TConsensusSpec::Addr>,
    ) -> Result<(), HotStuffError> {
        // The leader that is supposed to propose the next block timed out
        let timeout_height = current_height + NodeHeight(2);

        let (next_leader, high_pc, last_sent_vote) = self.store.with_read_tx(|tx| {
            let leaf_block = LeafBlock::get(tx, epoch)?;
            let next_leader = get_leader_for_view(
                tx,
                local_committee,
                &self.leader_strategy,
                leaf_block.block_id(),
                // Skipping the next height since the leader failed to propose
                timeout_height,
            )?;
            let high_pc = HighPc::get(tx, epoch)?;
            let high_pc = ProposalCertificate::get(tx, high_pc.id())?;
            let last_sent_vote = LastSentVote::get(tx, epoch)
                .optional()?
                .filter(|vote| high_pc.height() < vote.block_height());
            Ok::<_, HotStuffError>((next_leader, high_pc, last_sent_vote))
        })?;

        info!(
            target: LOG_TARGET,
            "🌟 Send NEWVIEW to {} {} HighPC: {} Vote[{}]",
            next_leader.address,
            timeout_height,
            high_pc,
            last_sent_vote.display(),
        );

        let msg = TimeoutVoteMessage {
            epoch: high_pc.epoch(),
            height: timeout_height,
        };

        let signature = self.signer_service.sign(&msg);
        let signature = ValidatorSignatureBytes::new(
            self.signer_service.public_key().to_byte_type(),
            signature.to_byte_type(),
        );

        let message = NewViewMessage {
            high_pc,
            last_vote: None, // last_sent_vote.map(|vote| vote.vote),
            timeout: TimeoutVote {
                epoch,
                height: timeout_height,
                signature,
            },
        };

        self.outbound_messaging
            .send(next_leader.address.clone(), HotstuffMessage::new_newview(message))
            .await?;

        Ok(())
    }
}
