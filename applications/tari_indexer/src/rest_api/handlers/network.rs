//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use axum::{Extension, Json, response::Response};
use libp2p::swarm::dial_opts::PeerCondition;
use ootle_byte_type::FromByteType;
use tari_indexer_client::{
    types,
    types::{
        AddPeerRequest,
        AddPeerResponse,
        ConnectionDirection,
        GetConnectionsResponse,
        GetNetworkInfoResponse,
        GetNetworkSyncStateResponse,
        NetworkDescription,
        SyncProgress,
    },
};
use tari_networking::{DialOpts, NetworkingService, is_supported_multiaddr};
use tari_ootle_common_types::{optional::Optional, public_key_to_peer_id};

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

#[utoipa::path(get, path = "/network/stats", description = "Get network sync stats")]
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
    };
    Ok(Json(response))
}

#[utoipa::path(get, path = "/network/connections", description = "Get active peer connections")]
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
            address: conn.endpoint.get_remote_address().clone(),
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

#[utoipa::path(post, path = "/network/connections", description = "Add a new peer connection")]
pub async fn add_connection(
    Extension(context): Extension<HandlerContext>,
    Json(req): Json<AddPeerRequest>,
) -> HandlerResult<Json<AddPeerResponse>> {
    let AddPeerRequest {
        public_key,
        addresses,
        wait_for_dial,
    } = req;

    if let Some(unsupported) = addresses.iter().find(|a| !is_supported_multiaddr(a)) {
        return Err(ErrorResponse::bad_request(format!(
            "Unsupported multiaddr {unsupported}"
        )));
    }

    let mut networking = context.networking().clone();
    let public_key = public_key
        .try_from_byte_type()
        .map_err(|_| ErrorResponse::bad_request("Public key is malformed"))?;
    let peer_id = public_key_to_peer_id(public_key);

    let dial_wait = networking
        .dial_peer(
            DialOpts::peer_id(peer_id)
                .addresses(addresses)
                .condition(PeerCondition::Always)
                .build(),
        )
        .await
        .map_err(|err| ErrorResponse::general_error(err.to_string()))?;

    if wait_for_dial {
        dial_wait
            .await
            .map_err(|err| ErrorResponse::general_error(format!("Failed to dial peer {}: {}", peer_id, err)))?;
    }

    Ok(Json(AddPeerResponse {}))
}
