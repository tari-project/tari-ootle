//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, sync::Arc, time::SystemTime};

use log::*;
use tari_consensus::hotstuff::ConsensusCurrentState;
use tari_ootle_common_types::{Epoch, NodeHeight, ShardGroup};
use tari_ootle_p2p::{PeerAddress, proto::rpc};
use tokio::sync::RwLock;

use crate::network_state_sync::committee_client::ValidatorRpcSession;

const LOG_TARGET: &str = "tari::indexer::network_state_sync::validator_status";

#[derive(Debug, Clone)]
pub struct ValidatorStatusSnapshot {
    pub shard_group: ShardGroup,
    pub epoch: Epoch,
    pub height: NodeHeight,
    pub state: ConsensusCurrentState,
    pub observed_at: SystemTime,
}

/// In-memory, lazily-populated map of the last observed consensus state of each
/// validator the indexer has contacted. Entries are timestamped so callers can
/// decide whether the data is fresh enough to be trustworthy.
#[derive(Debug, Clone, Default)]
pub struct ValidatorStatusMonitor {
    inner: Arc<RwLock<HashMap<PeerAddress, ValidatorStatusSnapshot>>>,
}

impl ValidatorStatusMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn snapshots(&self) -> Vec<(PeerAddress, ValidatorStatusSnapshot)> {
        let guard = self.inner.read().await;
        guard.iter().map(|(k, v)| (*k, v.clone())).collect()
    }

    /// Probe the validator at the other end of the given session for its current
    /// consensus state and record the result. Returns silently on any RPC error
    /// so that the caller's primary workflow is not disrupted.
    pub async fn probe(&self, session: &mut ValidatorRpcSession, shard_group: ShardGroup) {
        let peer = *session.peer_address();
        let resp = match session.get_consensus_state(rpc::GetConsensusStateRequest {}).await {
            Ok(r) => r,
            Err(e) => {
                debug!(
                    target: LOG_TARGET,
                    "Consensus state probe failed for validator {peer}: {e}"
                );
                return;
            },
        };

        let Some(epoch) = resp.epoch.map(Epoch::from) else {
            warn!(
                target: LOG_TARGET,
                "Consensus state response from validator {peer} is missing epoch"
            );
            return;
        };

        let state = match rpc::ConsensusState::try_from(resp.state) {
            Ok(s) => ConsensusCurrentState::from(s),
            Err(e) => {
                warn!(
                    target: LOG_TARGET,
                    "Consensus state response from validator {peer} has invalid state discriminant ({}): {e}",
                    resp.state
                );
                return;
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

        let mut guard = self.inner.write().await;
        guard.insert(peer, snapshot);
    }
}
