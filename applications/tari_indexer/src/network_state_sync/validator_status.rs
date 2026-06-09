//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, sync::Arc, time::SystemTime};

use log::*;
use tari_consensus::hotstuff::ConsensusCurrentState;
use tari_epoch_manager::{EpochManagerReader, service::EpochManagerHandle};
use tari_ootle_common_types::{Epoch, NodeHeight, ShardGroup, VotePower};
use tari_ootle_p2p::{PeerAddress, proto::rpc};
use tari_ootle_storage::consensus_models::{CommittedBlockProof, VerifiedBlockTip};
use tokio::sync::RwLock;

use crate::network_state_sync::committee_client::ValidatorRpcSession;

const LOG_TARGET: &str = "tari::indexer::network_state_sync::validator_status";

/// A snapshot of one validator's self-reported consensus pacemaker state, observed by the indexer
/// during a recent sync round.
///
/// This is diagnostic only and intentionally *unverified* - it backs the Validators page and is not
/// trusted for sync-source selection. The trust decision is made separately by verifying the peer's
/// committed block proof (see [`ValidatorStatusMonitor::probe`]).
#[derive(Debug, Clone)]
pub struct ValidatorStatusSnapshot {
    pub shard_group: ShardGroup,
    pub epoch: Epoch,
    pub height: NodeHeight,
    pub state: ConsensusCurrentState,
    pub observed_at: SystemTime,
}

/// In-memory, lazily-populated map of the last observed consensus state of each validator the
/// indexer has contacted. Entries are timestamped so callers can decide whether the data is fresh.
#[derive(Clone)]
pub struct ValidatorStatusMonitor {
    epoch_manager: EpochManagerHandle<PeerAddress>,
    inner: Arc<RwLock<HashMap<PeerAddress, ValidatorStatusSnapshot>>>,
}

impl ValidatorStatusMonitor {
    pub fn new(epoch_manager: EpochManagerHandle<PeerAddress>) -> Self {
        Self {
            epoch_manager,
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn snapshots(&self) -> Vec<(PeerAddress, ValidatorStatusSnapshot)> {
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
    /// As a side effect, the peer's self-reported consensus state is recorded for the diagnostic
    /// Validators page. That status is deliberately *not* trusted for sync-source selection.
    pub async fn probe(
        &self,
        session: &mut ValidatorRpcSession,
        shard_group: ShardGroup,
    ) -> Result<Option<VerifiedBlockTip>, ProbeError> {
        let peer = *session.peer_address();

        // The trust decision: verify how far the peer has actually committed. A forged proof gates
        // the peer out; other failures are tolerated. On success the verified tip is returned so the
        // caller can record the quorum-signed state root.
        let verified_tip = match self.verify_committed_tip(session).await {
            Ok(tip) => Some(tip),
            Err(e) => {
                if e.is_invalid_proof() {
                    return Err(e);
                }
                debug!(target: LOG_TARGET, "Could not verify committed tip for validator {peer}: {e}");
                None
            },
        };

        // Unverified consensus status for diagnostics. A failure here does not disqualify the peer;
        // it just means we have no fresh diagnostic snapshot to show.
        let resp = match session.get_consensus_state(rpc::GetConsensusStateRequest {}).await {
            Ok(r) => r,
            Err(e) => {
                debug!(target: LOG_TARGET, "Consensus state probe failed for validator {peer}: {e}");
                return Ok(verified_tip);
            },
        };

        let Some(epoch) = resp.epoch.map(Epoch::from) else {
            warn!(target: LOG_TARGET, "Consensus state response from validator {peer} is missing epoch");
            return Ok(verified_tip);
        };

        let state = match rpc::ConsensusState::try_from(resp.state) {
            Ok(s) => ConsensusCurrentState::from(s),
            Err(e) => {
                warn!(
                    target: LOG_TARGET,
                    "Consensus state response from validator {peer} has invalid state discriminant ({}): {e}",
                    resp.state
                );
                return Ok(verified_tip);
            },
        };

        let snapshot = ValidatorStatusSnapshot {
            shard_group,
            epoch,
            height: NodeHeight::from(resp.height),
            state,
            observed_at: SystemTime::now(),
        };

        debug!(
            target: LOG_TARGET,
            "Observed validator {peer} in {shard_group}: epoch={}, height={}, state={}",
            snapshot.epoch, snapshot.height, snapshot.state
        );

        self.inner.write().await.insert(peer, snapshot);
        Ok(verified_tip)
    }

    /// Fetch and verify the validator's latest committed block proof against its shard group
    /// committee. Verification is the basis for deciding whether to trust the peer as a sync source.
    async fn verify_committed_tip(&self, session: &mut ValidatorRpcSession) -> Result<VerifiedBlockTip, ProbeError> {
        let resp = session
            .get_committed_block_proof(rpc::GetCommittedBlockProofRequest {})
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

        proof
            .validate(committee.quorum_threshold(), |pk| {
                Ok(committee.get_power_by_public_key(pk).unwrap_or_else(VotePower::zero))
            })
            .map_err(|e| ProbeError::InvalidProof(e.to_string()))
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
