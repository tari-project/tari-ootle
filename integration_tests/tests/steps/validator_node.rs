//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::{
    fs,
    str::FromStr,
    time::{Duration, Instant},
};

use cucumber::{gherkin::Step, given, then, when};
use integration_tests::{
    TariWorld,
    base_node::get_base_node_client,
    cucumber_log,
    template,
    template::{RegisteredTemplate, send_template_registration},
    validator_node::{ValidatorNodeProcess, spawn_validator_node},
};
use libp2p::Multiaddr;
use log::warn;
use minotari_app_grpc::tari_rpc::{RegisterValidatorNodeRequest, Signature};
use notify::Watcher;
use tari_base_node_client::{BaseNodeClient, grpc::GrpcBaseNodeClient};
use tari_crypto::tari_utilities::ByteArray;
use tari_ootle_common_types::{
    Epoch,
    SubstateAddress,
    layer_one_transaction::LayerOneTransactionDef,
    optional::Optional,
};
use tari_ootle_storage::Ordering;
use tari_sidechain::EvictionProof;
use tari_transaction_components::transaction_components::{MemoField, memo_field::TxType};
use tari_validator_node_client::types::{
    AddPeerRequest,
    GetBlocksRequest,
    GetStateRequest,
    GetTemplateRequest,
    ListBlocksRequest,
};
use tokio::{sync::mpsc, time::timeout};
use tonic::codegen::tokio_stream::StreamExt;

async fn spawn_seed_node(
    world: &mut TariWorld,
    seed_vn_name: String,
    bn_name: String,
    claim_fee_account: Option<&str>,
) -> ValidatorNodeProcess {
    let validator = spawn_validator_node(world, seed_vn_name.clone(), bn_name, claim_fee_account).await;
    // Ensure any existing nodes know about the new seed node
    let mut client = validator.get_client();
    let ident = client.get_identity().await.unwrap();
    for vn in world.validator_nodes.values() {
        let mut client = vn.get_client();
        client
            .add_peer(AddPeerRequest {
                public_key: ident.public_key,
                addresses: ident.public_addresses.clone(),
                wait_for_dial: false,
            })
            .await
            .unwrap();
    }
    for indexer in world.indexers.values() {
        indexer.add_peer(ident.public_key, ident.public_addresses.clone()).await;
    }

    validator
}

#[given(expr = "a validator node {word} connected to base node {word}")]
async fn start_vn_without_claim_fee(world: &mut TariWorld, step: &Step, seed_vn_name: String, bn_name: String) {
    cucumber_log!("==== Step: {}", step.value);
    let validator = spawn_validator_node(world, seed_vn_name.clone(), bn_name, None).await;
    world.validator_nodes.insert(seed_vn_name, validator);
}

#[given(expr = "a seed validator node {word} connected to base node {word}")]
async fn start_seed_vn_without_claim_fee(world: &mut TariWorld, step: &Step, seed_vn_name: String, bn_name: String) {
    cucumber_log!("==== Step: {}", step.value);
    let validator = spawn_seed_node(world, seed_vn_name.clone(), bn_name, None).await;
    world.vn_seeds.insert(seed_vn_name, validator);
}

#[given(expr = "a seed validator node {word} connected to base node {word} using claim fee account {word}")]
async fn start_seed_validator_node(
    world: &mut TariWorld,
    step: &Step,
    seed_vn_name: String,
    bn_name: String,
    claim_fee_account: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let validator = spawn_seed_node(world, seed_vn_name.clone(), bn_name, Some(&claim_fee_account)).await;
    world.vn_seeds.insert(seed_vn_name, validator);
}

#[given(expr = "validator {word} nodes connect to all other validators")]
async fn given_validator_connects_to_other_vns(world: &mut TariWorld, step: &Step, name: String) {
    cucumber_log!("==== Step: {}", step.value);
    let details = world
        .all_running_validators_iter()
        .filter(|vn| vn.name != name)
        .map(|vn| {
            (
                vn.public_key,
                Multiaddr::from_str(&format!("/ip4/127.0.0.1/tcp/{}", vn.p2p_port)).unwrap(),
            )
        })
        .collect::<Vec<_>>();

    let vn = world.validator_nodes.get_mut(&name).unwrap();
    let mut cli = vn.create_client();
    for (pk, addr) in details {
        if let Err(err) = cli
            .add_peer(AddPeerRequest {
                public_key: pk,
                addresses: vec![addr],
                wait_for_dial: true,
            })
            .await
        {
            // TODO: investigate why this can fail. This call failing ("cannot assign requested address (os error 99)")
            // doesnt cause the rest of the test test to fail, so ignoring for now.
            eprintln!("Failed to add peer: {}", err);
        }
    }
}

#[when(expr = "validator node {word} sends a registration transaction to base wallet {word}")]
pub async fn send_vn_registration(world: &mut TariWorld, step: &Step, vn_name: String, base_wallet_name: String) {
    cucumber_log!("==== Step: {}", step.value);
    let vn = world.get_validator_node(&vn_name);

    let mut base_layer_wallet = world.get_wallet(&base_wallet_name).create_client().await;
    world.mark_point_in_logs("before get_registration_info");
    let info = vn.get_registration_info().await;
    let registration = info.payload;

    let response = base_layer_wallet
        .register_validator_node(RegisterValidatorNodeRequest {
            validator_node_public_key: registration.public_key.to_vec(),
            validator_node_signature: Some(Signature {
                public_nonce: registration.signature.public_nonce().as_bytes().to_vec(),
                signature: registration.signature.signature().as_bytes().to_vec(),
            }),
            max_epoch: registration.max_epoch.as_u64(),
            validator_node_claim_public_key: registration.claim_public_key.as_bytes().to_vec(),
            sidechain_deployment_key: registration
                .sidechain_public_key
                .map(|key| key.to_vec())
                .unwrap_or_default(),
            fee_per_gram: 1,
            payment_id: MemoField::new_open_from_string("Register by cucumber", TxType::ValidatorNodeR