//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{ops::Deref, time::UNIX_EPOCH};

use axum::{Extension, Json, response::Response};
use tari_consensus::hotstuff::ConsensusCurrentState;
use tari_indexer_client::{
    types,
    types::{
        ConnectionDirection,
        GetConnectionsResponse,
        GetNetworkInfoResponse,
        GetNetworkSyncStateResponse,
        NetworkDescription,
        SyncProgress,
        ValidatorConsensusState,
        ValidatorStatus,
    },
};
use tari_ootle_common_types::optional::Optional;

use crate::rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult};

#[utoipa::path(get, path = "/network", description = "Get network info")]
pub async fn get(Extension(context): Extension<HandlerContext>) -> HandlerResult<Json<GetNetworkInfoResponse>> {
    let epoch = context.epoch_manager().get_current_epoch();
    let network = context.network();

    let response = GetNetworkInfoResponse {
        epoch,
        network_byte: network.as_byte(),
        network,
    };
    Ok(Json(response))
}

#[utoipa::path(get, path = "/network/stats", description = "Get network sync stats",
    responses(
        (status = 200, body = GetNetworkSyncStateResponse),
        (status = INTERNAL_SERVER_ERROR, body = ErrorResponse),
    ),
)]
pub async fn get_network_sync_stats(
    Extension(context): Extension<HandlerContext>,
) -> HandlerResult<Json<GetNetworkSyncStateResponse>> {
    let network_desc = context
        .epoch_manager()
        .get_network_description()
        .await
        .map_err(ErrorResponse::anyhow)?;
    let sync_progress = context
        .read_only_store()
        .get_sync_progress()
        .optional()
        .map_err(ErrorResponse::anyhow)?;

    let validators = context
        .validator_status()
        .snapshots()
        .await
        .into_iter()
        .map(|(peer, snapshot)| {
            let observed_at_unix_s = snapshot
                .observed_at
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default();
            ValidatorStatus {
                peer_id: peer.to_string(),
                shard_group: snapshot.shard_group,
                epoch: snapshot.epoch,
                height: snapshot.height.as_u64(),
                state: to_client_consensus_state(snapshot.state),
                observed_at_unix_s,
            }
        })
        .collect();

    let response = GetNetworkSyncStateResponse {
        network_desc: NetworkDescription {
            epoch: network_desc.epoch,
            shard_groups: network_desc
                .shard_groups
                .into_iter()
                .map(|(shard_group, info)| (shard_group, info.num_members))
                .collect(),
            num_preshards: network_desc.num_preshards,
        },
        sync_progress: sync_progress.map(|p| SyncProgress {
            last_epoch: p.last_epoch,
            checkpoint_progress: p.checkpoint_progress.into_iter().collect(),
            last_state_versions: p.last_state_versions.into_iter().collect(),
        }),
        validators,
    };
    Ok(Json(response))
}

fn to_client_consensus_state(state: ConsensusCurrentState) -> ValidatorConsensusState {
    match state {
        ConsensusCurrentState::Idle => ValidatorConsensusState::Idle,
        ConsensusCurrentState::CheckSync => ValidatorConsensusState::CheckSync,
        ConsensusCurrentState::Syncing => ValidatorConsensusState::Syncing,
        ConsensusCurrentState::Running => ValidatorConsensusState::Running,
        ConsensusCurrentState::OnHold => ValidatorConsensusState::OnHold,
        ConsensusCurrentState::Sleeping => ValidatorConsensusState::Sleeping,
        ConsensusCurrentState::Shutdown => ValidatorConsensusState::Shutdown,
    }
}

#[utoipa::path(get, path = "/network/connections", description = "Get active peer connections",
    responses(
        (status = 200, body = GetConnectionsResponse),
        (status = INTERNAL_SERVER_ERROR, body = ErrorResponse),
    ),
)]
pub async fn get_connections(Extension(context): Extension<HandlerContext>) -> HandlerResult<Response> {
    let active_connections = context
        .networking()
        .get_active_connections()
        .await
        .map_err(ErrorResponse::anyhow)?;

    let connections = active_connections
        .into_iter()
        .map(|conn| types::Connection {
            connection_id: conn.connection_id.to_string(),
            peer_id: conn.peer_id.to_string(),
            // We use a string to avoid depending on multiaddr in the client
            address: conn.endpoint.get_remote_address().to_string(),
            direction: if conn.endpoint.is_dialer() {
                ConnectionDirection::Outbound
            } else {
                ConnectionDirection::Inbound
            },
            age: conn.age(),
            ping_latency: conn.ping_latency,
            user_agent: conn.user_agent.map(|arc| arc.deref().clone()),
        })
        .collect();

    Ok(context.apply_cache_control(Json(GetConnectionsResponse { connections }), 10))
}
