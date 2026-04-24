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
use tari_ootle_common_types::{Epoch, optional::Optional};
use tari_validator_node_client::types::{GetTransactionRequest, GetTransactionResultRequest};

#[when(expr = "I issue a rollback directive for target epoch {int} to all validators")]
async fn rollback_all_validators(world: &mut TariWorld, step: &Step, target_epoch: u64) {
    cucumber_log!("==== Step: {}", step.value);
    send_rollback_directive(world, Epoch(target_epoch)).await;
}

/// Rollback to the epoch immediately preceding whichever epoch the validators are in *now*.
/// Uses the first validator as the oracle for "now" — the test keeps L1 stable across this
/// step, so all VNs see the same current epoch.
#[when(expr = "I issue a rollback directive to the previous epoch to all validators")]
async fn rollback_to_previous_epoch(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);

    let vn = world
        .validator_nodes
        .values()
        .next()
        .expect("no validator nodes registered");
    let mut client = vn.create_client();
    let status = client
        .get_consensus_status()
        .await
        .unwrap_or_else(|e| panic!("get_consensus_status on {}: {e}", vn.name));
    let current = status.epoch.as_u64();
    assert!(
        current >= 1,
        "cannot roll back to previous epoch: current consensus epoch is {current}",
    );
    let target = Epoch(current - 1);
    cucumber_log!(
        "Deriving rollback target epoch={} from {} current epoch={}",
        target,
        vn.name,
        current,
    );
    send_rollback_directive(world, target).await;
}

async fn send_rollback_directive(world: &TariWorld, target_epoch: Epoch) {
    let body = DirectiveBody {
        kind: DirectiveKind::rollback_to_epoch(target_epoch),
        nonce: rand::random(),
        issued_at_unix_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };
    let directive =
        ConsensusDirective::sign(body, &world.governance_secret_key, &mut OsRng).expect("signing directive");

    let client = reqwest::Client::new();
    for vn in world.validator_nodes.values() {
        let url = vn.admin_json_rpc_url();
        let req_body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "admin.apply_consensus_directive",
            "params": { "directive": &directive },
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
        assert!(value.get("error").is_none(), "JSON-RPC error from {}: {value}", vn.name,);
        cucumber_log!("[{}] rollback directive accepted: {}", vn.name, value);
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

/// Asserts that the validator's consensus height is at most `max_height`. Used after a
/// rollback to check that a fresh genesis was installed (height near zero) rather than
/// asserting a single exact value — the genesis path may emit a dummy block or two before
/// the caller can observe state.
#[then(expr = "validator node {word} reports consensus height at most {int}")]
async fn validator_reports_height_at_most(world: &mut TariWorld, step: &Step, vn_name: String, max_height: u64) {
    cucumber_log!("==== Step: {}", step.value);
    let vn = world.get_validator_node(&vn_name);
    let mut client = vn.create_client();
    let status = client
        .get_consensus_status()
        .await
        .unwrap_or_else(|e| panic!("get_consensus_status on {vn_name}: {e}"));
    assert!(
        status.height.as_u64() <= max_height,
        "{vn_name} consensus height {} exceeds max {max_height}; status: epoch={} state={}",
        status.height,
        status.epoch,
        status.state,
    );
    cucumber_log!(
        "[{}] consensus height {} is within <= {max_height} (epoch={} state={})",
        vn_name,
        status.height,
        status.epoch,
        status.state,
    );
}

/// Asserts that the validator has a finalized execution for the named transaction — i.e.
/// the transaction has been committed into a block. Polls briefly because even after the
/// wallet daemon reports a result, remote validators may still be catching up.
#[then(expr = "validator node {word} has committed transaction {word}")]
async fn validator_has_committed_transaction(world: &mut TariWorld, step: &Step, vn_name: String, tx_name: String) {
    cucumber_log!("==== Step: {}", step.value);
    let transaction_id = *world
        .submitted_transactions
        .get(&tx_name)
        .unwrap_or_else(|| panic!("No submitted transaction recorded under name {tx_name}"));
    let vn = world.get_validator_node(&vn_name);
    let mut client = vn.create_client();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let maybe_result = client
            .get_transaction_result(GetTransactionResultRequest { transaction_id })
            .await
            .optional()
            .unwrap_or_else(|e| panic!("get_transaction_result on {vn_name}: {e}"));
        if let Some(result) = maybe_result {
            cucumber_log!(
                "[{}] transaction {} committed with decision {:?}",
                vn_name,
                transaction_id,
                result.final_decision,
            );
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("{vn_name} did not commit transaction {tx_name} ({transaction_id}) within timeout");
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Asserts that the validator has no record of the named transaction being committed.
/// After a rollback, the block that finalized the transaction was deleted along with its
/// execution record, so `get_transaction_result` returns NotFound. `get_transaction` may
/// still return the raw transaction envelope (it's not epoch-indexed), so we check the
/// *result* — "committed" is the rollback invariant under test.
#[then(expr = "validator node {word} has not committed transaction {word}")]
async fn validator_has_not_committed_transaction(
    world: &mut TariWorld,
    step: &Step,
    vn_name: String,
    tx_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let transaction_id = *world
        .submitted_transactions
        .get(&tx_name)
        .unwrap_or_else(|| panic!("No submitted transaction recorded under name {tx_name}"));
    let vn = world.get_validator_node(&vn_name);
    let mut client = vn.create_client();

    let maybe_result = client
        .get_transaction_result(GetTransactionResultRequest { transaction_id })
        .await
        .optional()
        .unwrap_or_else(|e| panic!("get_transaction_result on {vn_name}: {e}"));
    if let Some(result) = maybe_result {
        panic!(
            "{vn_name} still reports transaction {tx_name} ({transaction_id}) as committed with decision {:?}",
            result.final_decision,
        );
    }
    // The envelope record may or may not still exist — log which for debugging, but do not fail.
    let envelope = client
        .get_transaction(GetTransactionRequest { transaction_id })
        .await
        .optional()
        .unwrap_or_else(|e| panic!("get_transaction on {vn_name}: {e}"));
    cucumber_log!(
        "[{}] transaction {} is not committed (envelope record {})",
        vn_name,
        transaction_id,
        if envelope.is_some() { "still present" } else { "also removed" },
    );
}
