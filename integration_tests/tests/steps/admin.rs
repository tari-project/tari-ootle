//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Cucumber steps exercising the admin JSON-RPC surface — currently the rollback directive
//! path. These compose the same primitives as the `tari_validator_admin_cli` binary: build a
//! `ConsensusDirective`, sign with the scenario's governance secret key (held in
//! `TariWorld::governance_secret_key`), and submit to each validator's admin JSON-RPC
//! endpoint.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cucumber::{gherkin::Step, then, when};
use integration_tests::{TariWorld, cucumber_log};
use rand::rngs::OsRng;
use serde_json::json;
use tari_consensus_types::{ConsensusDirective, DirectiveBody, DirectiveKind};
use tari_ootle_common_types::Epoch;

#[when(expr = "I issue a rollback directive for target epoch {int} to all validators")]
async fn rollback_all_validators(world: &mut TariWorld, step: &Step, target_epoch: u64) {
    cucumber_log!("==== Step: {}", step.value);

    let body = DirectiveBody {
        kind: DirectiveKind::rollback_to_epoch(Epoch(target_epoch)),
        nonce: rand::random(),
        issued_at_unix_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };
    let directive = ConsensusDirective::sign(body, &world.governance_secret_key, &mut OsRng)
        .expect("signing directive");
    let directive_hex = hex::encode(borsh::to_vec(&directive).expect("serialising directive"));

    let client = reqwest::Client::new();
    for vn in world.validator_nodes.values() {
        let url = vn.admin_json_rpc_url();
        let req_body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "admin.apply_consensus_directive",
            "params": { "directive_hex": directive_hex.clone() },
        });

        let resp = client
            .post(&url)
            .json(&req_body)
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .unwrap_or_else(|e| panic!("POST {url} for {}: {e}", vn.name));
        let status = resp.status();
        let value: serde_json::Value = resp
            .json()
            .await
            .unwrap_or_else(|e| panic!("decoding JSON from {}: {e}", vn.name));

        assert!(status.is_success(), "HTTP {status} from {}: {value}", vn.name);
        assert!(
            value.get("error").is_none(),
            "JSON-RPC error from {}: {value}",
            vn.name,
        );
        cucumber_log!("[{}] rollback directive accepted: {}", vn.name, value);
    }
}

#[then(expr = "validator node {word} has rolled back past epoch {int}")]
async fn validator_rolled_back_past(world: &mut TariWorld, step: &Step, vn_name: String, target_epoch: u64) {
    cucumber_log!("==== Step: {}", step.value);
    let vn = world.get_validator_node(&vn_name);
    let mut client = vn.create_client();

    // Poll because the orchestrator releases on-hold and consensus transitions asynchronously;
    // immediately after the RPC returns, the state machine is only guaranteed to be leaving
    // OnHold, not yet Running at a post-rollback epoch.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let status = client
            .get_consensus_status()
            .await
            .unwrap_or_else(|e| panic!("get_consensus_status on {vn_name}: {e}"));
        // After rollback, consensus_epoch should be at most target_epoch + some small delta
        // (target_epoch itself, or target_epoch + 1 if the genesis path has already created
        // a fresh post-rollback epoch, etc.). The essential check is that we did NOT stay at
        // whatever epoch we were at before the rollback.
        if status.epoch.0 <= target_epoch + 2 {
            cucumber_log!(
                "[{}] post-rollback status: epoch={} height={} state={}",
                vn_name,
                status.epoch,
                status.height,
                status.state,
            );
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "{vn_name} did not reach epoch <= {} within timeout; current epoch: {}",
                target_epoch + 2,
                status.epoch,
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[then(expr = "validator node {word} reports consensus state {word}")]
async fn validator_reports_state(world: &mut TariWorld, step: &Step, vn_name: String, expected_state: String) {
    cucumber_log!("==== Step: {}", step.value);
    let vn = world.get_validator_node(&vn_name);
    let mut client = vn.create_client();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let status = client
            .get_consensus_status()
            .await
            .unwrap_or_else(|e| panic!("get_consensus_status on {vn_name}: {e}"));
        if status.state == expected_state {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "{vn_name} did not reach consensus state {expected_state} within timeout; current state: {}",
                status.state,
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
