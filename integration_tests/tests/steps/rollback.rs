//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Cucumber steps for the offline break-glass rollback tool.
//!
//! These steps compose two independently useful primitives:
//!   1. `I shut down validator node X`  — stops the in-process VN task and awaits it, guaranteeing the
//!      `Arc<TransactionDB>` clones have dropped and RocksDB's LOCK file is released (axum's graceful shutdown is what
//!      makes this complete in bounded time — see `json_rpc::server::spawn_json_rpc`).
//!   2. `I apply an offline rollback to epoch N on validator node X` — invokes the tool's library surface against the
//!      stopped validator's data dir.
//!
//! Keeping the two split mirrors the operator runbook (`systemctl stop; run tool`)
//! and lets other feature files reuse the shutdown step independently.

use std::{path::PathBuf, time::Duration};

use cucumber::{then, when};
use integration_tests::{TariWorld, validator_node::respawn_validator_node};
use multiaddr::multiaddr;
use tari_ootle_common_types::Epoch;
use tari_ootle_storage::{StateStore, StateStoreReadTransaction};
use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};
use tari_validator_node_client::types::AddPeerRequest;
use tari_validator_rollback::apply::{ApplyOptions, run_with_options};

#[when(expr = "I shut down validator node {word}")]
async fn shut_down_validator_node(world: &mut TariWorld, vn_name: String) {
    let vn = world.get_validator_node_mut(&vn_name);
    vn.stop_and_wait().await;
    let state_db_path = vn.state_db_path();
    assert!(
        state_db_path.exists(),
        "state db path {} does not exist after shutdown",
        state_db_path.display(),
    );
    integration_tests::cucumber_log!(
        "Validator node {} shut down (state db at {})",
        vn_name,
        state_db_path.display()
    );
}

#[when(expr = "I apply an offline rollback to epoch {int} on validator node {word}")]
async fn apply_offline_rollback_on_validator_node(world: &mut TariWorld, target_epoch: u64, vn_name: String) {
    let state_db_path: PathBuf = world.get_validator_node(&vn_name).state_db_path();
    let audit_out = state_db_path
        .parent()
        .expect("state db path has no parent")
        .join(format!("rollback-audit-{}.bin", target_epoch));

    let outcome = run_with_options(ApplyOptions {
        state_db: state_db_path.clone(),
        target_epoch,
        shard_group: None,
        audit_out: Some(audit_out.clone()),
        dry_run: false,
    })
    .expect("rollback apply failed");

    assert_eq!(
        outcome.target_epoch,
        Epoch(target_epoch),
        "outcome target epoch mismatch"
    );
    assert_eq!(outcome.audit_path, audit_out, "audit file path does not match request");
    assert!(
        audit_out.exists(),
        "audit file was not written at {}",
        audit_out.display()
    );

    integration_tests::cucumber_log!(
        "Rolled back {} to epoch {} — audit {} ({} substates_removed, {} substates_rewound, {} unfinalised tx, {} \
         blocks)",
        vn_name,
        target_epoch,
        audit_out.display(),
        outcome.substates_removed,
        outcome.substates_rewound,
        outcome.transactions_unfinalised,
        outcome.blocks_deleted,
    );
}

#[when(expr = "I start validator node {word}")]
async fn start_validator_node(world: &mut TariWorld, vn_name: String) {
    let vn = respawn_validator_node(world, vn_name.clone()).await;
    world.validator_nodes.insert(vn_name, vn);
}

#[when(expr = "validator nodes reconnect to each other")]
async fn validator_nodes_reconnect_peers(world: &mut TariWorld) {
    // After restart both VNs have fresh p2p ports, so whatever peer routing table they
    // had before is stale. Mirror `create_network`'s pairwise add_peer step so consensus
    // gossip can find its counterpart again.
    let snapshots: Vec<_> = world
        .validator_nodes
        .values()
        .map(|vn| (vn.public_key, vn.get_addresses(), vn.json_rpc_port))
        .collect();
    for (i, vn) in world.validator_nodes.values().enumerate() {
        let mut client = vn.create_client();
        for (j, (pk, addrs, _port)) in snapshots.iter().enumerate() {
            if i == j {
                continue;
            }
            client
                .add_peer(AddPeerRequest {
                    public_key: *pk,
                    addresses: addrs.clone(),
                    wait_for_dial: false,
                })
                .await
                .expect("add_peer");
        }
    }
    // Indexer still needs to know about the restarted VNs for wallet_daemon lookups.
    for vn in world.validator_nodes.values() {
        world
            .get_indexer(&world.indexers.keys().next().cloned().expect("indexer registered"))
            .add_peer(vn.public_key, vec![multiaddr!(Ip4([127, 0, 0, 1]), Tcp(vn.p2p_port))])
            .await;
    }
}

#[then(expr = "validator node {word} reports consensus state Running within {int} seconds")]
async fn validator_reports_running(world: &mut TariWorld, vn_name: String, timeout_secs: u64) {
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        let status = {
            let vn = world.get_validator_node(&vn_name);
            let mut client = vn.get_client();
            client.get_consensus_status().await.unwrap()
        };
        if status.state == "Running" {
            integration_tests::cucumber_log!("{vn_name} consensus Running at epoch {}", status.epoch);
            return;
        }
        if std::time::Instant::now() > deadline {
            panic!("{vn_name} did not reach consensus=Running within {timeout_secs}s (last: {status:?})");
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[then(expr = "validator node {word} has a rollback history entry at epoch {int}")]
async fn assert_rollback_history_has_entry_at_epoch(world: &mut TariWorld, vn_name: String, target_epoch: u64) {
    let state_db_path: PathBuf = world.get_validator_node(&vn_name).state_db_path();
    // Re-open the (now-stopped) state store to read the rollback_history CF. The tool
    // released the lock on write-tx commit, so this open succeeds.
    let store: RocksDbStateStore<tari_ootle_p2p::PeerAddress> =
        RocksDbStateStore::open(&state_db_path, DatabaseOptions::default())
            .expect("open state db to read rollback_history");
    let entries = store
        .with_read_tx(|tx| tx.rollback_history_list())
        .expect("list rollback_history");
    assert!(
        entries.iter().any(|e| e.target_epoch == Epoch(target_epoch)),
        "expected a rollback_history entry at epoch {target_epoch} on {vn_name}, got {entries:?}",
    );
}
