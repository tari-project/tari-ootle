//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, sync::Arc, time::SystemTime};

use log::*;
use tari_epoch_manager::{EpochManagerReader, service::EpochManagerHandle};
use tari_ootle_common_types::{Epoch, NodeHeight, ShardGroup, VotePower};
use tari_ootle_p2p::{PeerAddress, proto::rpc};
use tari_ootle_storage::consensus_models::CommittedBlockProof;
use tari_template_lib_types::Hash32;
use tokio::sync::RwLock;

use crate::network_state_sync::committee_client::ValidatorRpcSession;

const LOG_TARGET: &str = "tari::indexer::network_state_sync::validator_status";

/// A *verified* snapshot of how far a validator has committed, as proven to the indexer during a
/// recent sync round. Unlike a self-reported consensus state, every field here is authenticated by
/// the shard group committee's quorum signatures, so a byzantine or lagging validator cannot
/// inflate its apparent progress.
#[derive(Debug, Clone)]
pub struct ValidatorStatusSnapshot {
    pub shard_group: ShardGroup,
    pub epoch: Epoch,
    pub height: NodeHeight,
    /// State merkle root of the validator's latest committed block (verified).
    pub state_merkle_root: Hash32,
    pub observed_at: SystemTime,
}

/// In-memory, lazily-populated map of the last *verified* committed tip of each validator the
/// indexer has contacted. Entries are timestamped so callers can decide whether the data is fresh
/// enough to be trustworthy.
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

    /// Fetch and verify the validator's latest committed block proof, recording the verified tip.
    ///
    /// Unlike the previous unauthenticated consensus-state probe, this proves how far the peer has
    /// committed: the returned proof is validated against the shard group committee, so the peer
    /// cannot lie about its height or state root.
    ///
    /// Returns the verified snapshot on success. A [`ProbeError::InvalidProof`] is a strong
    /// negative signal — the peer served a forged or malformed proof — and callers should avoid
    /// syncing from it. Other errors are non-fatal (e.g. the peer simply has nothing committed yet,
    /// or the indexer lacks the committee for the proof's epoch).
    pub async fn probe(
        &self,
        session: &mut ValidatorRpcSession,
        shard_group: ShardGroup,
    ) -> Result<ValidatorStatusSnapshot, ProbeError> {
        let peer = *session.peer_address();

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

        let verified = proof
            .validate(committee.quorum_threshold(), |pk| {
                Ok(committee.get_power_by_public_key(pk).unwrap_or_else(VotePower::zero))
            })
            .map_err(|e| ProbeError::InvalidProof(e.to_string()))?;

        let snapshot = ValidatorStatusSnapshot {
            shard_group,
            epoch: verified.epoch,
            height: verified.height,
            state_merkle_root: verified.state_merkle_root.into_array().into(),
            observed_at: SystemTime::now(),
        };

        debug!(
            target: LOG_TARGET,
            "✅ Verified validator {peer} tip in {shard_group}: epoch={}, height={}, state_root={}",
            snapshot.epoch, snapshot.height, snapshot.state_merkle_root
        );

        self.inner.write().await.insert(peer, snapshot.clone());
        Ok(snapshot)
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
