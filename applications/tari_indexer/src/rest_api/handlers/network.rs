//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    ops::Deref,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{Extension, Json, response::Response};
use tari_consensus::hotstuff::ConsensusCurrentState;
use tari_indexer_client::{
    types,
    types::{
        ConnectionDirection,
        GetConnectionsResponse,
        GetNetworkEconomicsResponse,
        GetNetworkInfoResponse,
        GetNetworkSyncStateResponse,
        NetworkDescription,
        ScheduledBurnRate,
        ShardGroupSyncPhase,
        ShardGroupSyncStatus,
        ShardLevel,
        ShardMismatch,
        ShardObservedLeaf,
        ShardSnapshotProgress,
        ShardSyncState,
        ShardSyncStatus,
        ShardVersionAt,
        SyncActivityBucket,
        SyncBlame,
        SyncObservedTip,
        SyncProgress,
        SyncStoredCheckpoint,
        ValidatorConsensusState,
        ValidatorProbeError,
        ValidatorProbeErrorKind,
        ValidatorStatus,
        ValidatorStatusSnapshot,
    },
};
use tari_ootle_common_types::{Epoch, ShardGroup, StateVersion, optional::Optional, shard::Shard};
use tari_template_lib_types::Amount;

use crate::{
    exhaust_burn_rate::resolve_burn_rate_outlook,
    network_state_sync::{self, ACTIVITY_BUCKET, GroupPhase, ProbeFailure, SyncStatus},
    rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult},
};

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

#[utoipa::path(
    get,
    path = "/network/economics",
    description = "Get network-wide TARI economic totals (claimed, burned, fee volume, supply, fees claimable by validators, target rate and the rate changes scheduled ahead)",
    responses(
        (status = 200, description = "Network economic totals", body = GetNetworkEconomicsResponse),
        (status = INTERNAL_SERVER_ERROR, description = "Failed to fetch network economics", body = ErrorResponse),
    ),
)]
pub async fn get_economics(Extension(context): Extension<HandlerContext>) -> HandlerResult<Response> {
    let current_epoch = context.epoch_manager().get_current_epoch();
    let econ = context
        .read_only_store()
        .get_tari_economics()
        .await
        .map_err(ErrorResponse::anyhow)?;

    let outlook = resolve_burn_rate_outlook(context.substate_manager(), context.network(), current_epoch).await;
    // Supply nets the receipt-sourced burn against claimed (both advance on the state-sync frontier), matching
    // `get_xtr_total_supply`; `total_exhaust_burned` (header) is reported alongside as a cross-check.
    let total_supply = econ
        .total_claimed
        .checked_sub(econ.receipt_exhaust_burned)
        .unwrap_or_else(Amount::zero);

    Ok(context.apply_cache_control(
        Json(GetNetworkEconomicsResponse {
            current_epoch,
            total_claimed: econ.total_claimed,
            total_exhaust_burned: econ.total_exhaust_burned,
            fee_volume: econ.fee_volume,
            receipt_exhaust_burned: econ.receipt_exhaust_burned,
            total_supply,
            transaction_receipt_count: econ.transaction_receipt_count,
            validator_claimable_fees: econ.validator_claimable_fees,
            target_burn_rate_bps: outlook.current.as_bps(),
            scheduled_burn_rates: outlook
                .scheduled
                .into_iter()
                .map(|change| ScheduledBurnRate {
                    activation_epoch: change.activation_epoch,
                    rate_bps: change.rate_bps,
                })
                .collect(),
            burn_rate_retired_from: outlook.retired_from,
        }),
        60,
    ))
}

#[utoipa::path(get, path = "/network/stats", description = "Get network sync stats",
    responses(
        (status = 200, description = "Network sync stats", body = GetNetworkSyncStateResponse),
        (status = INTERNAL_SERVER_ERROR, description = "Failed to fetch network sync stats", body = ErrorResponse),
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
        .await
        .optional()
        .map_err(ErrorResponse::anyhow)?;

    let validators = context
        .validator_status()
        .records()
        .await
        .into_iter()
        .map(|(peer, record)| ValidatorStatus {
            peer_id: peer.to_string(),
            shard_group: record.shard_group,
            probed_at_unix_s: to_unix_secs(record.probed_at),
            probe_error: record.error.map(to_client_probe_error),
            snapshot: record.snapshot.map(|snapshot| ValidatorStatusSnapshot {
                epoch: snapshot.epoch,
                height: snapshot.height.as_u64(),
                state: to_client_consensus_state(snapshot.state),
                observed_at_unix_s: to_unix_secs(snapshot.observed_at),
            }),
        })
        .collect();

    let now = SystemTime::now();
    let status = context.sync_status().snapshot();
    let max_serve_lag = context.published_config().substate_cache_max_serve_lag;
    let shard_groups = network_desc.shard_groups_iter().collect::<Vec<_>>();
    let progress = sync_progress.clone().unwrap_or_default();
    let shards = shard_groups
        .iter()
        .enumerate()
        .flat_map(|(i, shard_group)| {
            (i == 0)
                .then_some(Shard::global())
                .into_iter()
                .chain(shard_group.shard_iter())
                .map(|shard| (*shard_group, shard))
        })
        .map(|(shard_group, shard)| {
            to_shard_sync_status(
                shard,
                shard_group,
                &status,
                &progress,
                context.shard_watermarks().confirmed(shard),
                max_serve_lag,
                now,
            )
        })
        .collect();
    let shard_group_statuses = shard_groups
        .iter()
        .enumerate()
        .map(|(i, shard_group)| to_shard_group_sync_status(*shard_group, i == 0, network_desc.epoch(), &status, now))
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
        shard_groups: shard_group_statuses,
        shards,
        activity_bucket_secs: ACTIVITY_BUCKET.as_secs(),
    };
    Ok(Json(response))
}

fn to_shard_group_sync_status(
    shard_group: ShardGroup,
    syncs_global_shard: bool,
    epoch: Epoch,
    status: &SyncStatus,
    now: SystemTime,
) -> ShardGroupSyncStatus {
    let group = status.groups.get(&shard_group);
    let mut activity = network_state_sync::ActivityWindow::default()
        .buckets(now)
        .into_iter()
        .map(|bucket| SyncActivityBucket {
            start_unix_s: bucket.start,
            versions: 0,
            transactions: 0,
        })
        .collect::<Vec<_>>();
    let shards = syncs_global_shard
        .then_some(Shard::global())
        .into_iter()
        .chain(shard_group.shard_iter())
        .filter_map(|shard| status.shards.get(&shard));
    for shard in shards {
        for (total, bucket) in activity.iter_mut().zip(shard.promoted.buckets(now)) {
            total.versions += bucket.versions;
            total.transactions += bucket.transactions;
        }
    }

    ShardGroupSyncStatus {
        shard_group,
        phase: group
            .map(|group| to_client_phase(group.phase))
            .unwrap_or(ShardGroupSyncPhase::Starting),
        phase_since_unix_s: group.map(|group| to_unix_secs(group.phase_since)).unwrap_or_default(),
        stream_peer: group.and_then(|group| group.stream_peer).map(|peer| peer.to_string()),
        blames: group
            .map(|group| {
                group
                    .blamed
                    .iter()
                    .map(|blame| SyncBlame {
                        peer_ids: blame.peers.iter().map(|peer| peer.to_string()).collect(),
                        epoch: blame.epoch,
                        at_unix_s: to_unix_secs(blame.at),
                        details: blame.details.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        excluded_peers: group
            .filter(|group| group.excluded.0 == epoch)
            .map(|group| group.excluded.1.iter().map(|peer| peer.to_string()).collect())
            .unwrap_or_default(),
        tip: group.and_then(|group| group.tip).map(|tip| SyncObservedTip {
            epoch: tip.epoch,
            height: tip.height.as_u64(),
            observed_at_unix_s: to_unix_secs(tip.observed_at),
        }),
        last_checkpoint: group
            .and_then(|group| group.last_checkpoint)
            .map(|(epoch, stored_at)| SyncStoredCheckpoint {
                epoch,
                stored_at_unix_s: to_unix_secs(stored_at),
            }),
        activity,
    }
}

fn to_client_phase(phase: GroupPhase) -> ShardGroupSyncPhase {
    match phase {
        GroupPhase::Starting => ShardGroupSyncPhase::Starting,
        GroupPhase::SyncingCheckpoints => ShardGroupSyncPhase::SyncingCheckpoints,
        GroupPhase::Snapshotting { shard } => ShardGroupSyncPhase::Snapshotting { shard },
        GroupPhase::Streaming => ShardGroupSyncPhase::Streaming,
        GroupPhase::Paused { reason, until } => ShardGroupSyncPhase::Paused {
            reason: reason.to_string(),
            until_unix_s: to_unix_secs(until),
        },
        GroupPhase::Halted { shard, from } => ShardGroupSyncPhase::Halted {
            shard,
            from_version: from,
        },
    }
}

fn to_shard_sync_status(
    shard: Shard,
    shard_group: ShardGroup,
    status: &SyncStatus,
    progress: &network_state_sync::SyncProgress,
    level: Option<(StateVersion, Duration)>,
    max_serve_lag: Duration,
    now: SystemTime,
) -> ShardSyncStatus {
    let record = status.shards.get(&shard);
    let halted = status
        .groups
        .get(&shard_group)
        .is_some_and(|group| matches!(group.phase, GroupPhase::Halted { .. }));
    let snapshot = record.and_then(|record| record.snapshot);
    let verified = progress.verified.get(&shard).map(|verified| ShardVersionAt {
        state_version: verified.state_version,
        epoch: verified.epoch,
    });
    let staged = progress
        .last_state_versions
        .get(&shard)
        .map(|(state_version, epoch)| ShardVersionAt {
            state_version: *state_version,
            epoch: *epoch,
        });
    let state = shard_sync_state(
        halted,
        snapshot.is_some(),
        verified.is_some() || staged.is_some(),
        level.is_some_and(|(_, age)| age <= max_serve_lag),
    );
    ShardSyncStatus {
        shard,
        state,
        verified,
        staged,
        tip_leaf: record
            .and_then(|record| record.tip_leaf)
            .map(|(state_version, observed_at)| ShardObservedLeaf {
                state_version,
                observed_at_unix_s: to_unix_secs(observed_at),
            }),
        level: level.map(|(state_version, age)| ShardLevel {
            state_version,
            confirmed_secs_ago: age.as_secs(),
        }),
        last_promoted_at_unix_s: record.and_then(|record| record.last_promoted_at).map(to_unix_secs),
        versions_last_minute: record
            .map(|record| record.promoted.versions_within(now, Duration::from_secs(60)))
            .unwrap_or_default(),
        last_mismatch: record
            .and_then(|record| record.last_mismatch.as_ref())
            .map(|(at, details)| ShardMismatch {
                at_unix_s: to_unix_secs(*at),
                details: details.clone(),
            }),
        snapshot: snapshot.map(|snapshot| ShardSnapshotProgress {
            state_version: snapshot.state_version,
            substates: snapshot.substates as u64,
            fraction: snapshot.key_fraction,
            started_at_unix_s: to_unix_secs(snapshot.started_at),
        }),
    }
}

/// A shard is following while the stream's last finding that it is level with its committee is recent enough for the
/// substate cache to serve it, which it confirms only once every version up to that level is promoted.
fn shard_sync_state(halted: bool, snapshotting: bool, started: bool, level_is_fresh: bool) -> ShardSyncState {
    if halted {
        ShardSyncState::Halted
    } else if snapshotting {
        ShardSyncState::Snapshotting
    } else if !started {
        ShardSyncState::Waiting
    } else if level_is_fresh {
        ShardSyncState::Following
    } else {
        ShardSyncState::CatchingUp
    }
}

fn to_client_probe_error(failure: ProbeFailure) -> ValidatorProbeError {
    match failure {
        ProbeFailure::StatusUnavailable(message) => ValidatorProbeError {
            kind: ValidatorProbeErrorKind::StatusUnavailable,
            message,
        },
        ProbeFailure::InvalidProof(message) => ValidatorProbeError {
            kind: ValidatorProbeErrorKind::InvalidProof,
            message,
        },
    }
}

fn to_unix_secs(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}

fn to_client_consensus_state(state: ConsensusCurrentState) -> ValidatorConsensusState {
    match state {
        ConsensusCurrentState::Initialising => ValidatorConsensusState::Initialising,
        ConsensusCurrentState::Idle => ValidatorConsensusState::Idle,
        ConsensusCurrentState::CheckSync => ValidatorConsensusState::CheckSync,
        ConsensusCurrentState::Syncing => ValidatorConsensusState::Syncing,
        ConsensusCurrentState::Running => ValidatorConsensusState::Running,
        ConsensusCurrentState::Sleeping => ValidatorConsensusState::Sleeping,
        ConsensusCurrentState::Shutdown => ValidatorConsensusState::Shutdown,
    }
}

#[utoipa::path(get, path = "/network/connections", description = "Get active peer connections",
    responses(
        (status = 200, description = "Active peer connections", body = GetConnectionsResponse),
        (status = INTERNAL_SERVER_ERROR, description = "Failed to fetch peer connections", body = ErrorResponse),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_halted_group_halts_every_shard_state() {
        assert_eq!(shard_sync_state(true, true, true, true), ShardSyncState::Halted);
    }

    #[test]
    fn a_snapshot_outranks_progress() {
        assert_eq!(
            shard_sync_state(false, true, false, false),
            ShardSyncState::Snapshotting
        );
    }

    #[test]
    fn a_shard_with_nothing_staged_is_waiting() {
        assert_eq!(shard_sync_state(false, false, false, true), ShardSyncState::Waiting);
    }

    #[test]
    fn a_shard_follows_only_while_its_level_is_fresh() {
        assert_eq!(shard_sync_state(false, false, true, true), ShardSyncState::Following);
        assert_eq!(shard_sync_state(false, false, true, false), ShardSyncState::CatchingUp);
    }
}
