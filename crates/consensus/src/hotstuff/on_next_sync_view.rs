//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use log::*;
use ootle_byte_type::ToByteType;
use tari_consensus_types::{
    HighPc,
    HighTc,
    LastSentNewView,
    LastSentVote,
    ProposalCertificate,
    TimeoutVote,
    TimeoutVoteMessage,
    ValidatorSignatureBytes,
};
use tari_ootle_common_types::{Epoch, NodeHeight, committee::Committee, displayable::Displayable, optional::Optional};
use tari_ootle_storage::{StateStore, StateStoreReadTransaction, StorageError, consensus_models::BookkeepingModel};
use tari_ootle_transaction::Network;

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
    /// The timeout votes this node sent for the view it last named.
    last_sent: Option<SentTimeoutVotes>,
}

/// The timeout votes sent for one view. Each is re-sent unchanged for as long as it still applies, so a member that
/// already holds it can tell a re-send from a vote signed again.
#[derive(Debug, Clone)]
struct SentTimeoutVotes {
    /// The vote first signed for the view, sent to every member other than the leader. Those members count it for
    /// pacing only, which never reads its `high_pc_height` claim, so it stays the same after a higher certificate
    /// arrives.
    to_members: TimeoutVote,
    /// The vote in the NEWVIEW to the leader, which checks its claim against the certificate the NEWVIEW carries.
    to_leader: TimeoutVote,
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
            last_sent: None,
        }
    }

    /// Sends the NEWVIEW for the view after `current_height` that the committee has not yet timed out, and returns
    /// that view. `timeout_floor` is the lowest view this node may name, from the timeouts it has counted or joined.
    pub async fn handle(
        &mut self,
        epoch: Epoch,
        current_height: NodeHeight,
        timeout_floor: NodeHeight,
        local_committee: &Committee<TConsensusSpec::Addr>,
    ) -> Result<NodeHeight, HotStuffError> {
        let (next_leader, high_pc, last_sent_vote, timeout_height) = self.store.with_read_tx(|tx| {
            let timeout_height = next_timeout_height(tx, epoch, current_height, timeout_floor)?;
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
        let sent = self.timeout_votes(epoch, timeout_height, high_pc_height);
        self.last_sent = Some(sent.clone());
        let SentTimeoutVotes {
            to_members: timeout,
            to_leader: leader_timeout,
        } = sent;
        let local_public_key = timeout.signature.public_key;
        let message = NewViewMessage {
            high_pc,
            last_vote: last_sent_vote.map(|vote| vote.vote),
            timeout: leader_timeout,
        };

        self.outbound_messaging
            .send(next_leader.clone(), HotstuffMessage::new_newview(message))
            .await?;

        // Every member counts timeout votes to learn when this view has timed out, which takes 2f+1 of them, so
        // the vote goes to the whole committee. Only the leader proposes on a timeout certificate, so only the
        // leader needs the certificate that `high_pc_height` claims.
        let mut is_leader = false;
        let others = local_committee
            .iter()
            .filter(|member| {
                let is_local = member.public_key == local_public_key;
                is_leader |= is_local && member.address == *next_leader;
                !is_local && member.address != *next_leader
            })
            .map(|member| member.address.clone())
            .collect::<Vec<_>>();
        self.outbound_messaging
            .multicast(others, HotstuffMessage::TimeoutVote(timeout.clone()))
            .await?;
        if !is_leader {
            self.outbound_messaging
                .send_self(HotstuffMessage::TimeoutVote(timeout))
                .await?;
        }

        self.store.with_write_tx(|tx| {
            LastSentNewView {
                epoch,
                height: timeout_height,
            }
            .set(tx)
        })?;

        Ok(timeout_height)
    }

    /// The votes to send for view `height`, reusing those already sent for it while they still apply.
    fn timeout_votes(&self, epoch: Epoch, height: NodeHeight, high_pc_height: NodeHeight) -> SentTimeoutVotes {
        let Some(sent) = self
            .last_sent
            .as_ref()
            .filter(|sent| sent.to_members.epoch == epoch && sent.to_members.height == height)
        else {
            let vote = self.sign_timeout(epoch, height, high_pc_height);
            return SentTimeoutVotes {
                to_members: vote.clone(),
                to_leader: vote,
            };
        };
        let to_leader = if sent.to_leader.high_pc_height == high_pc_height {
            sent.to_leader.clone()
        } else {
            self.sign_timeout(epoch, height, high_pc_height)
        };
        SentTimeoutVotes {
            to_members: sent.to_members.clone(),
            to_leader,
        }
    }

    fn sign_timeout(&self, epoch: Epoch, height: NodeHeight, high_pc_height: NodeHeight) -> TimeoutVote {
        sign_timeout_vote(&self.signer_service, self.config.network, epoch, height, high_pc_height)
    }
}

/// This node's timeout vote for view `height`, reporting a high certificate at `high_pc_height`.
pub(super) fn sign_timeout_vote<TSigner: ValidatorSignerService>(
    signer: &TSigner,
    network: Network,
    epoch: Epoch,
    height: NodeHeight,
    high_pc_height: NodeHeight,
) -> TimeoutVote {
    let msg = TimeoutVoteMessage::new(network, epoch, height, high_pc_height);
    let signature = signer.sign(&msg);
    TimeoutVote {
        epoch,
        height,
        high_pc_height,
        signature: ValidatorSignatureBytes::new(signer.public_key().to_byte_type(), signature.to_byte_type()),
    }
}

/// The view a NEWVIEW sent from `current_height` names. Current height is the highest seen view, +1 is the leader that
/// failed and +2 is the next leader that should propose. A view the committee has already timed out moves it on:
/// `HighTc` is the highest timeout certificate this node holds, and `timeout_floor` is the lowest view its counted
/// and joined timeouts allow. Each input is a certificate or the votes of the committee, never a count of this node's
/// own timeouts, so members that hold the same certificates name the same view however long the view stalls.
fn next_timeout_height<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    epoch: Epoch,
    current_height: NodeHeight,
    timeout_floor: NodeHeight,
) -> Result<NodeHeight, StorageError> {
    let after_high_tc = HighTc::get(tx, epoch)
        .optional()?
        .map_or(NodeHeight::zero(), |tc| tc.height().saturating_add(NodeHeight(1)));
    Ok(current_height
        .saturating_add(NodeHeight(2))
        .max(after_high_tc)
        .max(timeout_floor))
}

#[cfg(test)]
mod tests {
    use tari_consensus_types::TcId;
    use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};

    use super::*;

    const EPOCH: Epoch = Epoch(1);

    type TestStore = RocksDbStateStore<String>;

    fn open(path: &std::path::Path) -> TestStore {
        TestStore::open(path.join("db"), DatabaseOptions::default()).unwrap()
    }

    fn timeout_height(path: &std::path::Path, current_height: NodeHeight, timeout_floor: NodeHeight) -> NodeHeight {
        open(path)
            .with_read_tx(|tx| next_timeout_height(tx, EPOCH, current_height, timeout_floor))
            .unwrap()
    }

    fn persist_high_tc(path: &std::path::Path, epoch: Epoch, height: NodeHeight) {
        open(path)
            .with_write_tx(|tx| {
                HighTc {
                    epoch,
                    height,
                    tc_id: TcId::zero(),
                }
                .set(tx)
            })
            .unwrap();
    }

    #[test]
    fn a_first_newview_skips_the_failed_leader() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            timeout_height(dir.path(), NodeHeight(10), NodeHeight::zero()),
            NodeHeight(12)
        );
    }

    #[test]
    fn repeated_timeouts_name_the_same_view() {
        let dir = tempfile::tempdir().unwrap();
        open(dir.path())
            .with_write_tx(|tx| {
                LastSentNewView {
                    epoch: EPOCH,
                    height: NodeHeight(12),
                }
                .set(tx)
            })
            .unwrap();
        assert_eq!(
            timeout_height(dir.path(), NodeHeight(10), NodeHeight::zero()),
            NodeHeight(12)
        );
    }

    #[test]
    fn a_timeout_certificate_moves_the_view_past_it() {
        let dir = tempfile::tempdir().unwrap();
        persist_high_tc(dir.path(), EPOCH, NodeHeight(20));
        assert_eq!(
            timeout_height(dir.path(), NodeHeight(10), NodeHeight::zero()),
            NodeHeight(21)
        );
    }

    #[test]
    fn a_timeout_certificate_from_another_epoch_does_not_move_the_view() {
        let dir = tempfile::tempdir().unwrap();
        persist_high_tc(dir.path(), Epoch(0), NodeHeight(20));
        assert_eq!(
            timeout_height(dir.path(), NodeHeight(10), NodeHeight::zero()),
            NodeHeight(12)
        );
    }

    #[test]
    fn the_timeout_floor_moves_the_view() {
        let dir = tempfile::tempdir().unwrap();
        persist_high_tc(dir.path(), EPOCH, NodeHeight(20));
        assert_eq!(
            timeout_height(dir.path(), NodeHeight(10), NodeHeight(30)),
            NodeHeight(30)
        );
    }

    #[test]
    fn timeout_height_saturates_at_node_height_max() {
        let dir = tempfile::tempdir().unwrap();
        persist_high_tc(dir.path(), EPOCH, NodeHeight::max());
        assert_eq!(
            timeout_height(dir.path(), NodeHeight::max(), NodeHeight::zero()),
            NodeHeight::max()
        );
    }
}
