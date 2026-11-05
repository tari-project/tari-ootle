//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeSet, HashMap, hash_map::Entry},
    sync::Arc,
    time::SystemTime,
};

use log::*;
use ootle_network::Network;
use tari_consensus::hotstuff::ConsensusCurrentState;
use tari_epoch_manager::{EpochManagerReader, service::EpochManagerHandle};
use tari_ootle_common_types::{Epoch, NodeHeight, ShardGroup, StateVersion, shard::Shard};
use tari_ootle_p2p::{PeerAddress, proto::rpc};
use tari_ootle_storage::consensus_models::{CommittedBlockProof, ShardStateLeaf, VerifiedBlockTip};
use tari_state_tree::TreeHash;
use tokio::sync::RwLock;

use crate::network_state_sync::{committee_client::ValidatorRpcSession, verified_stage::VerifiedShardLeaf};

const LOG_TARGET: &str = "tari::indexer::network_state_sync::validator_status";

/// A snapshot of one validator's self-reported consensus pacemaker state, observed by the indexer
/// during a recent sync round.
///
/// This is diagnostic only and intentionally *unverified* - it backs the Validators page and is not
/// trusted for sync-source selection. The trust decision is made separately by verifying the peer's
/// committed block proof (see [`ValidatorStatusMonitor::probe`]).
#[derive(Debug, Clone)]
pub struct ValidatorStatusSnapshot {
    pub epoch: Epoch,
    pub height: NodeHeight,
    pub state: ConsensusCurrentState,
    pub observed_at: SystemTime,
}

/// The outcome of the most recent probe of one validator, alongside the last snapshot any probe of
/// it produced.
#[derive(Debug, Clone)]
pub struct ValidatorProbeRecord {
    pub shard_group: ShardGroup,
    pub probed_at: SystemTime,
    /// Why the most recent probe produced no snapshot, or `None` when it produced `snapshot`.
    pub error: Option<ProbeFailure>,
    /// The last snapshot a probe produced. It predates `probed_at` whenever `error` is set.
    pub snapshot: Option<ValidatorStatusSnapshot>,
}

impl ValidatorProbeRecord {
    fn from_outcome(
        shard_group: ShardGroup,
        probed_at: SystemTime,
        outcome: Result<ValidatorStatusSnapshot, ProbeFailure>,
    ) -> Self {
        let (snapshot, error) = match outcome {
            Ok(snapshot) => (Some(snapshot), None),
            Err(e) => (None, Some(e)),
        };
        Self {
            shard_group,
            probed_at,
            error,
            snapshot,
        }
    }

    /// Records a probe's outcome. A failed probe keeps the previous snapshot, now stale.
    fn apply(
        &mut self,
        shard_group: ShardGroup,
        probed_at: SystemTime,
        outcome: Result<ValidatorStatusSnapshot, ProbeFailure>,
    ) {
        self.shard_group = shard_group;
        self.probed_at = probed_at;
        match outcome {
            Ok(snapshot) => {
                self.error = None;
                self.snapshot = Some(snapshot);
            },
            Err(e) => self.error = Some(e),
        }
    }
}

/// Why a probe of a validator produced no snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeFailure {
    /// The validator did not return a usable consensus state.
    StatusUnavailable(String),
    /// The validator served a committed block proof that failed verification, so it is not
    /// trusted as a sync source.
    InvalidProof(String),
}

/// A validator's committed tip whose commit proof verified, with the shard leaves it proved against the same block.
#[derive(Debug, Clone)]
pub struct VerifiedProbe {
    pub tip: VerifiedBlockTip,
    /// The requested shards' leaves, in ascending shard order. A shard outside the validator's shard group has
    /// none, and there are none at all when the tip is of another shard group than the one probed.
    pub leaves: Vec<ProvedShardLeaf>,
}

/// A shard's leaf as a probed block's state merkle root commits it, with the root of the shard's tree the leaf holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProvedShardLeaf {
    pub leaf: VerifiedShardLeaf,
    pub shard_root: TreeHash,
}

/// In-memory, lazily-populated map of the latest probe of each validator the indexer has contacted.
#[derive(Clone)]
pub struct ValidatorStatusMonitor {
    network: Network,
    epoch_manager: EpochManagerHandle<PeerAddress>,
    inner: Arc<RwLock<HashMap<PeerAddress, ValidatorProbeRecord>>>,
}

impl ValidatorStatusMonitor {
    pub fn new(network: Network, epoch_manager: EpochManagerHandle<PeerAddress>) -> Self {
        Self {
            network,
            epoch_manager,
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn records(&self) -> Vec<(PeerAddress, ValidatorProbeRecord)> {
        let guard = self.inner.read().await;
        guard.iter().map(|(k, v)| (*k, v.clone())).collect()
    }

    /// Probe a validator before trusting it as a sync source.
    ///
    /// The meaningful check is the verification of the peer's latest committed block proof against
    /// the shard group committee: a forged or malformed proof yields [`ProbeError::InvalidProof`] so
    /// callers can refuse to sync from that peer. Other verification failures are non-fatal (e.g.
    /// the peer has nothing committed yet).
    ///
    /// The leaves of `leaf_shards` are proved against the same block, and a leaf that fails to verify is an invalid
    /// proof like a forged commit proof.
    ///
    /// As a side effect, the peer's self-reported consensus state is recorded for the diagnostic
    /// Validators page. That status is deliberately *not* trusted for sync-source selection.
    pub async fn probe(
        &self,
        session: &mut ValidatorRpcSession,
        shard_group: ShardGroup,
        leaf_shards: &BTreeSet<Shard>,
    ) -> Result<Option<VerifiedProbe>, ProbeError> {
        let peer = *session.peer_address();

        // The trust decision: verify how far the peer has actually committed. A forged proof gates
        // the peer out; other failures are tolerated. On success the verified tip is returned so the
        // caller can record the quorum-signed state root.
        let verified_tip = match self.verify_committed_tip(session, shard_group, leaf_shards).await {
            Ok(probe) => Some(probe),
            Err(e) => {
                if e.is_invalid_proof() {
                    self.record(peer, shard_group, Err(ProbeFailure::InvalidProof(e.to_string())))
                        .await;
                    return Err(e);
                }
                debug!(target: LOG_TARGET, "Could not verify committed tip for validator {peer}: {e}");
                None
            },
        };

        // Unverified consensus status for diagnostics. A failure here does not disqualify the peer;
        // it is recorded against the peer so the Validators page can show why its snapshot is stale.
        let snapshot = Self::fetch_consensus_state(session).await;
        match &snapshot {
            Ok(snapshot) => debug!(
                target: LOG_TARGET,
                "Observed validator {peer} in {shard_group}: epoch={}, height={}, state={}",
                snapshot.epoch, snapshot.height, snapshot.state
            ),
            Err(e) => warn!(target: LOG_TARGET, "Consensus state probe failed for validator {peer}: {e}"),
        }
        self.record(peer, shard_group, snapshot.map_err(ProbeFailure::StatusUnavailable))
            .await;
        Ok(verified_tip)
    }

    async fn fetch_consensus_state(session: &mut ValidatorRpcSession) -> Result<ValidatorStatusSnapshot, String> {
        let resp = session
            .get_consensus_state(rpc::GetConsensusStateRequest {})
            .await
            .map_err(|e| format!("consensus state request failed: {e}"))?;
        let epoch = resp
            .epoch
            .map(Epoch::from)
            .ok_or_else(|| "consensus state response is missing epoch".to_string())?;
        let state = rpc::ConsensusState::try_from(resp.state).map_err(|e| {
            format!(
                "consensus state response has invalid state discriminant ({}): {e}",
                resp.state
            )
        })?;
        Ok(ValidatorStatusSnapshot {
            epoch,
            height: NodeHeight::from(resp.height),
            state: ConsensusCurrentState::from(state),
            observed_at: SystemTime::now(),
        })
    }

    async fn record(
        &self,
        peer: PeerAddress,
        shard_group: ShardGroup,
        outcome: Result<ValidatorStatusSnapshot, ProbeFailure>,
    ) {
        let now = SystemTime::now();
        match self.inner.write().await.entry(peer) {
            Entry::Occupied(entry) => entry.into_mut().apply(shard_group, now, outcome),
            Entry::Vacant(entry) => {
                entry.insert(ValidatorProbeRecord::from_outcome(shard_group, now, outcome));
            },
        }
    }

    /// Fetch and verify the validator's latest committed block proof against its shard group
    /// committee, and the leaves of `leaf_shards` against its block. Verification is the basis for deciding whether
    /// to trust the peer as a sync source.
    async fn verify_committed_tip(
        &self,
        session: &mut ValidatorRpcSession,
        shard_group: ShardGroup,
        leaf_shards: &BTreeSet<Shard>,
    ) -> Result<VerifiedProbe, ProbeError> {
        let resp = session
            .get_committed_block_proof(rpc::GetCommittedBlockProofRequest {
                leaf_shards: leaf_shards.iter().map(|shard| shard.as_u32()).collect(),
            })
            .await
            .map_err(|e| ProbeError::Rpc(e.to_string()))?;

        let proof = CommittedBlockProof::from_bytes(&resp.commit_proof)
            .map_err(|e| ProbeError::InvalidProof(format!("undecodable commit proof: {e}")))?;

        let epoch = proof.epoch();
        let proof_shard_group = proof
            .shard_group()
            .map_err(|e| ProbeError::InvalidProof(format!("invalid shard group in proof header: {e}")))?;

        // The committee for the proof's epoch/shard group is what is expected to have signed it.
        // Verifying against the wrong committee will (correctly) fail the quorum check below.
        let committee = self
            .epoch_manager
            .get_committee_by_shard_group(epoch, proof_shard_group)
            .await
            .map_err(|e| ProbeError::CommitteeUnavailable(e.to_string()))?;

        let tip = proof
            .validate(self.network, committee.quorum_threshold(), &*committee)
            .map_err(|e| ProbeError::InvalidProof(e.to_string()))?;
        let leaves = verify_shard_leaves(self.network, &proof, shard_group, leaf_shards, resp.shard_leaves)?;
        Ok(VerifiedProbe { tip, leaves })
    }
}

/// Decodes `leaves` and verifies each against `proof`'s block. Each must be of a shard in `requested`, in strictly
/// ascending shard order. They are dropped once verified when the block is of a shard group other than
/// `probed_shard_group`: every shard group commits its own copy of the global shard, so only the probed group's block
/// proves the copy that group's stream follows.
fn verify_shard_leaves(
    network: Network,
    proof: &CommittedBlockProof,
    probed_shard_group: ShardGroup,
    requested: &BTreeSet<Shard>,
    leaves: Vec<rpc::ShardStateLeaf>,
) -> Result<Vec<ProvedShardLeaf>, ProbeError> {
    let mut previous = None;
    let leaves = leaves
        .into_iter()
        .map(|leaf| {
            let leaf = ShardStateLeaf::try_from(leaf)
                .map_err(|e| ProbeError::InvalidProof(format!("undecodable shard leaf: {e}")))?;
            if !requested.contains(&leaf.shard) {
                return Err(ProbeError::InvalidProof(format!(
                    "served a leaf of {}, which was not requested",
                    leaf.shard
                )));
            }
            if previous >= Some(leaf.shard) {
                return Err(ProbeError::InvalidProof(format!(
                    "served the leaf of {} out of shard order",
                    leaf.shard
                )));
            }
            previous = Some(leaf.shard);
            leaf.verify(network, proof)
                .map_err(|e| ProbeError::InvalidProof(format!("shard leaf: {e}")))?;
            Ok(ProvedShardLeaf {
                leaf: VerifiedShardLeaf {
                    shard: leaf.shard,
                    state_version: StateVersion::new(leaf.state_version),
                    transition_acc: leaf.transition_acc,
                },
                shard_root: leaf.shard_root,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let block_shard_group = proof
        .shard_group()
        .map_err(|e| ProbeError::InvalidProof(format!("invalid shard group in proof header: {e}")))?;
    if block_shard_group == probed_shard_group {
        Ok(leaves)
    } else {
        Ok(Vec::new())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("RPC error: {0}")]
    Rpc(String),
    #[error("committee unavailable for proof epoch/shard group: {0}")]
    CommitteeUnavailable(String),
    #[error("validator served an invalid committed block proof: {0}")]
    InvalidProof(String),
}

impl ProbeError {
    /// True when the peer served a cryptographically invalid (forged or malformed) proof, as
    /// opposed to merely being unreachable or not yet having anything committed.
    pub fn is_invalid_proof(&self) -> bool {
        matches!(self, ProbeError::InvalidProof(_))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn shard_group() -> ShardGroup {
        ShardGroup::new(1u32, 256u32)
    }

    fn unavailable() -> ProbeFailure {
        ProbeFailure::StatusUnavailable("consensus state request failed".to_string())
    }

    fn snapshot(height: u64, observed_at: SystemTime) -> ValidatorStatusSnapshot {
        ValidatorStatusSnapshot {
            epoch: Epoch(7),
            height: NodeHeight::from(height),
            state: ConsensusCurrentState::Running,
            observed_at,
        }
    }

    #[test]
    fn a_failed_probe_keeps_the_last_snapshot_and_records_the_error() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let t1 = t0 + Duration::from_secs(60);
        let mut record = ValidatorProbeRecord::from_outcome(shard_group(), t0, Ok(snapshot(10, t0)));

        record.apply(shard_group(), t1, Err(unavailable()));

        assert_eq!(record.probed_at, t1);
        assert_eq!(record.error, Some(unavailable()));
        let snapshot = record.snapshot.as_ref().unwrap();
        assert_eq!(snapshot.height, NodeHeight::from(10));
        assert_eq!(snapshot.observed_at, t0);
    }

    mod shard_leaves {
        use tari_engine_types::ProtocolVersion;
        use tari_state_tree::{ShardGroupRootTree, TransitionAccumulator};

        use super::*;

        const NETWORK: Network = Network::LocalNet;

        fn group() -> ShardGroup {
            ShardGroup::new(1u32, 4u32)
        }

        fn shard(n: u32) -> Shard {
            Shard::from(n)
        }

        /// A commit proof of a block of [`group`], and every leaf it commits, as served on the wire.
        fn served_block() -> (CommittedBlockProof, Vec<rpc::ShardStateLeaf>) {
            let protocol_version = ProtocolVersion::at(NETWORK, Epoch(1));
            let states = [
                (Shard::global(), 0u8),
                (shard(1), 1),
                (shard(2), 2),
                (shard(3), 0),
                (shard(4), 4),
            ]
            .into_iter()
            .map(|(shard, n)| {
                (
                    shard,
                    TreeHash::new([n; 32]),
                    u64::from(n),
                    TransitionAccumulator::new([n.wrapping_add(100); 32]),
                )
            })
            .collect::<Vec<_>>();
            let tree = ShardGroupRootTree::build(protocol_version, states.clone()).unwrap();
            let proof = CommittedBlockProof::new(tari_sidechain::SidechainBlockCommitProof {
                header: tari_sidechain::SidechainBlockHeader {
                    network: NETWORK.as_byte(),
                    protocol_version: protocol_version.as_u32(),
                    parent_id: Default::default(),
                    justify_id: Default::default(),
                    height: 4,
                    epoch: 1,
                    epoch_hash: Default::default(),
                    shard_group: tari_sidechain::ShardGroup {
                        start: group().start().as_u32(),
                        end_inclusive: group().end().as_u32(),
                    },
                    proposed_by: Default::default(),
                    state_merkle_root: tari_sidechain::FixedHash::new(tree.root().into_array()),
                    command_merkle_root: Default::default(),
                    transaction_merkle_root: Default::default(),
                    signature: Default::default(),
                    accumulated_data: Default::default(),
                    metadata_hash: Default::default(),
                },
                proof_elements: vec![],
            });
            let leaves = states
                .into_iter()
                .map(|(shard, shard_root, state_version, transition_acc)| {
                    let leaf = ShardStateLeaf {
                        shard,
                        state_version,
                        shard_root,
                        transition_acc,
                        shard_root_proof: tree.get_proof(shard).unwrap().1,
                    };
                    rpc::ShardStateLeaf::try_from(&leaf).unwrap()
                })
                .collect();
            (proof, leaves)
        }

        fn requested(shards: &[Shard]) -> BTreeSet<Shard> {
            shards.iter().copied().collect()
        }

        fn rejection(result: Result<Vec<ProvedShardLeaf>, ProbeError>) -> String {
            match result {
                Err(ProbeError::InvalidProof(details)) => details,
                other => panic!("expected an invalid proof, got {other:?}"),
            }
        }

        #[test]
        fn served_leaves_of_requested_shards_verify() {
            let (proof, leaves) = served_block();
            let all = requested(&[Shard::global(), shard(1), shard(2), shard(3), shard(4)]);

            let verified = verify_shard_leaves(NETWORK, &proof, group(), &all, leaves).unwrap();

            let verified = verified
                .iter()
                .map(|ProvedShardLeaf { leaf, shard_root }| {
                    (
                        leaf.shard,
                        leaf.state_version.as_u64(),
                        leaf.transition_acc,
                        *shard_root,
                    )
                })
                .collect::<Vec<_>>();
            let expected = |shard, n: u8| {
                (
                    shard,
                    u64::from(n),
                    TransitionAccumulator::new([n + 100; 32]),
                    TreeHash::new([n; 32]),
                )
            };
            assert_eq!(verified, vec![
                expected(Shard::global(), 0),
                expected(shard(1), 1),
                expected(shard(2), 2),
                expected(shard(3), 0),
                expected(shard(4), 4),
            ]);
        }

        #[test]
        fn leaves_of_another_shard_groups_block_are_dropped_once_verified() {
            let (proof, leaves) = served_block();
            let all = requested(&[Shard::global(), shard(1), shard(2), shard(3), shard(4)]);
            let other = ShardGroup::new(1u32, 8u32);

            let verified = verify_shard_leaves(NETWORK, &proof, other, &all, leaves).unwrap();
            assert!(verified.is_empty());

            let (proof, mut leaves) = served_block();
            leaves[1].transition_acc = vec![7; 32];
            rejection(verify_shard_leaves(NETWORK, &proof, other, &all, leaves));
        }

        #[test]
        fn a_leaf_whose_accumulator_differs_from_the_committed_one_is_an_invalid_proof() {
            let (proof, mut leaves) = served_block();
            leaves.truncate(3);
            leaves[2].transition_acc = vec![7; 32];

            let details = rejection(verify_shard_leaves(
                NETWORK,
                &proof,
                group(),
                &requested(&[Shard::global(), shard(1), shard(2)]),
                leaves,
            ));
            assert!(details.starts_with("shard leaf:"), "{details}");
        }

        #[test]
        fn a_leaf_of_an_unrequested_shard_is_an_invalid_proof() {
            let (proof, mut leaves) = served_block();
            leaves.truncate(3);

            let details = rejection(verify_shard_leaves(
                NETWORK,
                &proof,
                group(),
                &requested(&[shard(1), shard(2)]),
                leaves,
            ));
            assert!(details.contains("not requested"), "{details}");
        }

        #[test]
        fn leaves_out_of_shard_order_or_repeated_are_an_invalid_proof() {
            let all = requested(&[Shard::global(), shard(1), shard(2), shard(3), shard(4)]);
            let (proof, mut leaves) = served_block();
            leaves.swap(1, 2);
            let details = rejection(verify_shard_leaves(NETWORK, &proof, group(), &all, leaves));
            assert!(details.contains("out of shard order"), "{details}");

            let (proof, mut leaves) = served_block();
            leaves[2] = leaves[1].clone();
            let details = rejection(verify_shard_leaves(NETWORK, &proof, group(), &all, leaves));
            assert!(details.contains("out of shard order"), "{details}");
        }
    }

    #[test]
    fn a_successful_probe_clears_the_error() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let t1 = t0 + Duration::from_secs(60);
        let mut record = ValidatorProbeRecord::from_outcome(shard_group(), t0, Err(unavailable()));
        assert!(record.snapshot.is_none());

        record.apply(shard_group(), t1, Ok(snapshot(11, t1)));

        assert_eq!(record.error, None);
        assert_eq!(record.snapshot.as_ref().unwrap().height, NodeHeight::from(11));
    }
}
