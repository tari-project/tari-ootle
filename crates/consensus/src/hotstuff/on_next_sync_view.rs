//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use log::*;
use ootle_byte_type::ToByteType;
use tari_consensus_types::{
    HighPc,
    LastSentNewView,
    LastSentVote,
    ProposalCertificate,
    TimeoutVote,
    TimeoutVoteMessage,
    ValidatorSignatureBytes,
};
use tari_ootle_common_types::{Epoch, NodeHeight, committee::Committee, displayable::Displayable, optional::Optional};
use tari_ootle_storage::{StateStore, StateStoreReadTransaction, StorageError, consensus_models::BookkeepingModel};

use crate::{
    hotstuff::{HotStuffError, HotstuffConfig, LeaderSkipSet},
    messages::{HotstuffMessage, NewViewMessage},
    traits::{CertificateStore, ConsensusSpec, OutboundMessaging, ValidatorSignerService},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_next_sync_view";

pub struct OnNextSyncViewHandler<TConsensusSpec: ConsensusSpec> {
    config: HotstuffConfig,
    store: TConsensusSpec::StateStore,
    outbound_messaging: TConsensusSpec::OutboundMessaging,
    leader_strategy: TConsensusSpec::LeaderStrategy,
    signer_service: TConsensusSpec::SignerService,
}

impl<TConsensusSpec: ConsensusSpec> OnNextSyncViewHandler<TConsensusSpec> {
    pub fn new(
        config: HotstuffConfig,
        store: TConsensusSpec::StateStore,
        outbound_messaging: TConsensusSpec::OutboundMessaging,
        leader_strategy: TConsensusSpec::LeaderStrategy,
        signer_service: TConsensusSpec::SignerService,
    ) -> Self {
        Self {
            config,
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
        let (next_leader, high_pc, last_sent_vote, timeout_height) = self.store.with_read_tx(|tx| {
            let timeout_height = next_timeout_height(tx, epoch, current_height)?;
            let high_pc = HighPc::get(tx, epoch)?;
            let high_pc = ProposalCertificate::get(tx, epoch, high_pc.id())?;
            // Skipping the next height since the leader failed to propose. The certificate we report
            // is the one the next proposal will carry, so it anchors the liveness state that decides
            // who that proposer is - and it is in the message, so the receiver checks itself against
            // the same anchor.
            //
            // This is the one selection site whose anchor is local rather than committed: at the
            // moment of a leader failure, replicas that received the failed view's block hold a
            // certificate one height above those that did not. A liveness state that changes on
            // exactly that boundary sends the committee's NEWVIEWs to two leaders, neither reaches
            // 2f+1 and the view is lost. That is accepted: it costs one more timeout, the next view
            // re-rolls the anchor, and there is nothing every sender agrees on at a timeout to
            // anchor on instead.
            let skip_set = LeaderSkipSet::load_for_justify(
                tx,
                epoch,
                high_pc.height(),
                local_committee,
                &self.config.consensus_constants.liveness_thresholds(),
            )?;
            let (next_leader, _) = skip_set.effective_leader(&self.leader_strategy, local_committee, timeout_height);
            let last_sent_vote = LastSentVote::get(tx, epoch)
                .optional()?
                .filter(|vote| high_pc.height() < vote.block_height());

            Ok::<_, HotStuffError>((next_leader, high_pc, last_sent_vote, timeout_height))
        })?;

        info!(
            target: LOG_TARGET,
            "🌟 Send NEWVIEW to {} {} HighPC: {} Vote[{}]",
            next_leader,
            timeout_height,
            high_pc,
            last_sent_vote.display(),
        );

        let high_pc_height = high_pc.height();
        let msg = TimeoutVoteMessage::new(self.config.network, epoch, timeout_height, high_pc_height);

        let signature = self.signer_service.sign(&msg);
        let signature = ValidatorSignatureBytes::new(
            self.signer_service.public_key().to_byte_type(),
            signature.to_byte_type(),
        );

        let message = NewViewMessage {
            high_pc,
            last_vote: last_sent_vote.map(|vote| vote.vote),
            timeout: TimeoutVote {
                epoch,
                height: timeout_height,
                high_pc_height,
                signature,
            },
        };

        self.outbound_messaging
            .send(next_leader.clone(), HotstuffMessage::new_newview(message))
            .await?;

        self.store.with_write_tx(|tx| {
            LastSentNewView {
                epoch,
                height: timeout_height,
            }
            .set(tx)
        })?;

        Ok(())
    }
}

/// The view a NEWVIEW sent from `current_height` names. Current height is the highest seen view, +1 is the leader that
/// failed and +2 is the next leader that should propose. After consecutive leader failures each NEWVIEW names a view
/// above the last one sent in this epoch, which is read from the store so that it holds across a restart.
fn next_timeout_height<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    epoch: Epoch,
    current_height: NodeHeight,
) -> Result<NodeHeight, StorageError> {
    let timeout_height = current_height + NodeHeight(2);
    let last_sent = LastSentNewView::get(tx, epoch).optional()?;
    Ok(last_sent
        .filter(|last_sent| last_sent.height >= timeout_height)
        .map_or(timeout_height, |last_sent| last_sent.height + NodeHeight(1)))
}

#[cfg(test)]
mod tests {
    use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};

    use super::*;

    const EPOCH: Epoch = Epoch(1);

    type TestStore = RocksDbStateStore<String>;

    fn timeout_height_after_reopen(path: &std::path::Path, current_height: NodeHeight) -> NodeHeight {
        let store = TestStore::open(path.join("db"), DatabaseOptions::default()).unwrap();
        store
            .with_read_tx(|tx| next_timeout_height(tx, EPOCH, current_height))
            .unwrap()
    }

    fn persist_last_sent(path: &std::path::Path, epoch: Epoch, height: NodeHeight) {
        let store = TestStore::open(path.join("db"), DatabaseOptions::default()).unwrap();
        store
            .with_write_tx(|tx| LastSentNewView { epoch, height }.set(tx))
            .unwrap();
    }

    #[test]
    fn a_first_newview_skips_the_failed_leader() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(timeout_height_after_reopen(dir.path(), NodeHeight(10)), NodeHeight(12));
    }

    #[test]
    fn a_newview_after_a_restart_names_a_view_above_the_last_one_sent() {
        let dir = tempfile::tempdir().unwrap();
        persist_last_sent(dir.path(), EPOCH, NodeHeight(13));
        assert_eq!(timeout_height_after_reopen(dir.path(), NodeHeight(10)), NodeHeight(14));
    }

    #[test]
    fn a_newview_sent_in_another_epoch_does_not_raise_the_view() {
        let dir = tempfile::tempdir().unwrap();
        persist_last_sent(dir.path(), Epoch(0), NodeHeight(13));
        assert_eq!(timeout_height_after_reopen(dir.path(), NodeHeight(10)), NodeHeight(12));
    }
}
