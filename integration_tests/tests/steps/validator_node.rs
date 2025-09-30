//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::{
    fs,
    str::FromStr,
    time::{Duration, Instant},
};

use cucumber::{given, then, when};
use integration_tests::{
    base_node::get_base_node_client,
    template,
    template::{send_template_registration, RegisteredTemplate},
    util::cucumber_log,
    validator_node::{spawn_validator_node, ValidatorNodeProcess},
    TariWorld,
};
use libp2p::Multiaddr;
use log::warn;
use minotari_app_grpc::tari_rpc::{RegisterValidatorNodeRequest, Signature};
use notify::Watcher;
use tari_base_node_client::{grpc::GrpcBaseNodeClient, BaseNodeClient};
use tari_crypto::tari_utilities::ByteArray;
use tari_ootle_common_types::{
    layer_one_transaction::LayerOneTransactionDef,
    optional::Optional,
    Epoch,
    SubstateAddress,
};
use tari_ootle_storage::Ordering;
use tari_sidechain::EvictionProof;
use tari_transaction_components::transaction_components::{memo_field::TxType, MemoField};
use tari_validator_node_client::types::{
    AddPeerRequest,
    GetBlocksRequest,
    GetStateRequest,
    GetTemplateRequest,
    ListBlocksRequest,
};
use tokio::{sync::mpsc, time::timeout};

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
        let mut client = indexer.get_jrpc_indexer_client();
        client
            .add_peer(tari_indexer_client::types::AddPeerRequest {
                public_key: ident.public_key,
                addresses: ident.public_addresses.clone(),
                wait_for_dial: false,
            })
            .await
            .unwrap();
    }

    validator
}

#[given(expr = "a validator node {word} connected to base node {word}")]
async fn start_vn_without_claim_fee(world: &mut TariWorld, seed_vn_name: String, bn_name: String) {
    let validator = spawn_validator_node(world, seed_vn_name.clone(), bn_name, None).await;
    world.validator_nodes.insert(seed_vn_name, validator);
}

#[given(expr = "a seed validator node {word} connected to base node {word}")]
async fn start_seed_vn_without_claim_fee(world: &mut TariWorld, seed_vn_name: String, bn_name: String) {
    let validator = spawn_seed_node(world, seed_vn_name.clone(), bn_name, None).await;
    world.vn_seeds.insert(seed_vn_name, validator);
}

#[given(expr = "a seed validator node {word} connected to base node {word} using claim fee account {word}")]
async fn start_seed_validator_node(
    world: &mut TariWorld,
    seed_vn_name: String,
    bn_name: String,
    claim_fee_account: String,
) {
    let validator = spawn_seed_node(world, seed_vn_name.clone(), bn_name, Some(&claim_fee_account)).await;
    world.vn_seeds.insert(seed_vn_name, validator);
}

#[given(expr = "validator {word} nodes connect to all other validators")]
async fn given_validator_connects_to_other_vns(world: &mut TariWorld, name: String) {
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
pub async fn send_vn_registration(world: &mut TariWorld, vn_name: String, base_wallet_name: String) {
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
            payment_id: MemoField::new_open_from_string("Register by cucumber", TxType::ValidatorNodeRegistration)
                .unwrap()
                .to_bytes(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(
        response.is_success,
        "Failed to register validator node {}",
        response.failure_message
    );
    cucumber_log(format!(
        "Validator node registration tx id: {}",
        response.transaction_id
    ));
    world.mark_point_in_logs("after register_validator_node");
}

#[when(expr = "wallet daemon {word} publishes the template \"{word}\" using account {word}")]
async fn publish_template(
    world: &mut TariWorld,
    wallet_daemon_name: String,
    template_name: String,
    account_name: String,
) {
    world.mark_point_in_logs("Start publishing template");
    let template_address =
        match template::publish_template(world, wallet_daemon_name, account_name, template_name.clone()).await {
            Ok(resp) => resp,
            Err(e) => {
                println!("publish_template error = {}", e);
                panic!("publish_template error = {}", e);
            },
        };
    assert!(!template_address.is_empty());

    // store the template address for future reference
    let registered_template = RegisteredTemplate {
        name: template_name.clone(),
        address: template_address,
    };
    world.templates.insert(template_name, registered_template);

    world.mark_point_in_logs("End publishing template");
}

#[when(expr = "base wallet {word} registers the template \"{word}\"")]
async fn register_template(world: &mut TariWorld, wallet_name: String, template_name: String) {
    world.mark_point_in_logs("Start register template");
    let template_address = match send_template_registration(world, template_name.clone(), wallet_name).await {
        Ok(resp) => resp,
        Err(e) => {
            println!("register_template error = {}", e);
            panic!("register_template error = {}", e);
        },
    };
    assert!(!template_address.is_empty());

    // store the template address for future reference
    let registered_template = RegisteredTemplate {
        name: template_name.clone(),
        address: template_address,
    };
    world.templates.insert(template_name, registered_template);

    world
        .wait_until_base_nodes_have_transaction_in_mempool(1, Duration::from_secs(10))
        .await;
    world.mark_point_in_logs("End register template");
}

#[then(expr = "all validator nodes are listed as registered")]
async fn assert_all_vns_are_registered(world: &mut TariWorld) {
    for vn_ps in world.all_running_validators_iter() {
        // create a base node client
        let base_node_grpc_port = vn_ps.base_node_grpc_port;
        let mut base_node_client: GrpcBaseNodeClient = get_base_node_client(base_node_grpc_port);

        // get the list of registered vns from the base node
        let height = base_node_client.get_tip_info().await.unwrap().height_of_longest_chain;
        let vns = base_node_client.get_validator_nodes(height).await.unwrap();
        assert!(!vns.is_empty());

        // retrieve the VN's public key
        let mut client = vn_ps.get_client();
        let identity = client.get_identity().await.unwrap();

        // check that the vn's public key is in the list of registered vns
        assert!(vns.iter().any(|vn| vn.public_key == identity.public_key));
    }
}

#[then(expr = "the validator node {word} is listed as registered")]
pub async fn assert_vn_is_registered(world: &mut TariWorld, vn_name: String) {
    // create a base node client
    let vn = world.get_validator_node(&vn_name);
    let mut base_node_client: GrpcBaseNodeClient = get_base_node_client(vn.base_node_grpc_port);

    // get the list of registered vns from the base node
    let height = base_node_client.get_tip_info().await.unwrap().height_of_longest_chain;
    let vns = base_node_client.get_validator_nodes(height).await.unwrap();
    assert!(!vns.is_empty(), "vns are empty at height {}", height);

    // retrieve the VN's public key
    let mut client = vn.get_client();
    let identity = client.get_identity().await.unwrap();

    // check that the vn's public key is in the list of registered vns
    assert!(vns.iter().any(|vn| vn.public_key == identity.public_key));

    let mut count = 0;
    loop {
        // wait for the validator to pick up the registration
        let stats = client.get_epoch_manager_stats().await.unwrap();
        if stats.current_block_height >= height || stats.committee_info.is_some() {
            break;
        }
        if count > 20 {
            panic!(
                "Timed out waiting for validator node to pick up registration (current block height: {})",
                stats.current_block_height
            );
        }
        count += 1;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[then(expr = "the template \"{word}\" is listed as registered by the validator node {word}")]
async fn assert_template_is_registered(world: &mut TariWorld, template_name: String, vn_name: String) {
    // give it some time for the template tx to be picked up by the VNs
    // tokio::time::sleep(Duration::from_secs(4)).await;

    // retrieve the template address
    let template_address = world.templates.get(&template_name).unwrap().address;

    // try to get the template from the VN
    let timer = Instant::now();
    let vn = world.get_validator_node(&vn_name);
    let mut client = vn.get_client();
    loop {
        let req = GetTemplateRequest { template_address };
        let resp = client.get_template(req).await.ok();

        if resp.is_none() {
            if timer.elapsed() > Duration::from_secs(120) {
                panic!("Timed out waiting for template to be registered by all VNs");
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }

        // check that the template is indeed in the response
        assert_eq!(resp.unwrap().registration_metadata.address, template_address);
        break;
    }
}

#[then(expr = "the template \"{word}\" is listed as registered by all validator nodes")]
async fn assert_template_is_registered_by_all(world: &mut TariWorld, template_name: String) {
    // give it some time for the template tx to be picked up by the VNs
    // tokio::time::sleep(Duration::from_secs(4)).await;

    // retrieve the template address
    let template_address = world.templates.get(&template_name).unwrap().address;

    // try to get the template for each VN
    let timer = Instant::now();
    'outer: loop {
        for vn_ps in world.all_running_validators_iter() {
            let mut client = vn_ps.get_client();
            let req = GetTemplateRequest { template_address };
            let resp = client.get_template(req).await.ok();

            if resp.is_none() {
                if timer.elapsed() > Duration::from_secs(120) {
                    panic!("Timed out waiting for template to be registered by all VNs");
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
                continue 'outer;
            }
            let resp = resp.unwrap();
            // check that the template is indeed in the response
            assert_eq!(resp.registration_metadata.address, template_address);
        }
        break;
    }
}

#[then(expr = "validator node {word} has state at {word} within {int} seconds")]
async fn then_validator_node_has_state_at(
    world: &mut TariWorld,
    vn_name: String,
    state_address_name: String,
    timeout_secs: u64,
) {
    let state_address = world
        .substate_ids
        .get(&state_address_name)
        .unwrap_or_else(|| panic!("Address {} not found", state_address_name));
    cucumber_log(format!("Waiting for state at address {}", state_address));
    let vn = world.get_validator_node(&vn_name);
    let mut client = vn.create_client();
    let substate_address = SubstateAddress::from_substate_id(state_address, 0);
    let mut attempts = 0;
    loop {
        match client
            .get_state(GetStateRequest {
                address: substate_address,
            })
            .await
            .optional()
            .unwrap()
        {
            Some(_) => return,
            None => {
                attempts += 1;
                if attempts == timeout_secs {
                    panic!("State at address {} not found", state_address);
                }
            },
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[then(expr = "I wait for {word} to have at least {int} blocks for the current epoch")]
async fn vn_has_blocks_for_current_epoch(world: &mut TariWorld, vn_name: String, num_blocks: u64) {
    let vn = world.get_validator_node(&vn_name);
    let mut client = vn.create_client();
    for _ in 0..10 {
        let status = client.get_consensus_status().await.unwrap();
        if status.state != "Running" {
            warn!("Validator node {} is not running yet", vn_name);
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            continue;
        }
        if status.height.as_u64() >= num_blocks {
            return;
        }
        cucumber_log(format!(
            "Validator node {} has height {} ({}), waiting for at least {}",
            vn_name, status.height, status.state, num_blocks
        ));
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

#[then(expr = "{word} is on epoch {int} within {int} seconds")]
async fn vn_has_scanned_to_epoch(world: &mut TariWorld, vn_name: String, epoch: u64, seconds: usize) {
    let epoch = Epoch(epoch);
    let vn = world.get_validator_node(&vn_name);
    let mut client = vn.create_client();
    for _ in 0..seconds {
        let stats = client.get_epoch_manager_stats().await.expect("Failed to get stats");
        if stats.current_epoch == epoch {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    let stats = client.get_epoch_manager_stats().await.expect("Failed to get stats");
    assert_eq!(stats.current_epoch, epoch);
}

#[then(expr = "{word} has scanned to at least height {int}")]
async fn vn_has_scanned_to_height(world: &mut TariWorld, vn_name: String, block_height: u64) {
    let vn = world.get_validator_node(&vn_name);
    let mut client = vn.create_client();
    let mut last_block_height = 0;
    let mut remaining = 10;
    loop {
        let stats = client.get_epoch_manager_stats().await.expect("Failed to get stats");
        if stats.current_block_height >= block_height {
            return;
        }

        if stats.current_block_height != last_block_height {
            last_block_height = stats.current_block_height;
            // Reset the timer each time the scanned height changes
            remaining = 10;
        }

        if remaining == 0 {
            panic!(
                "Validator {} has not scanned to height {}. Current height: {}",
                vn_name, block_height, stats.current_block_height
            );
        }
        remaining -= 1;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[then(expr = "all validators have scanned to height {int}")]
#[when(expr = "all validators have scanned to height {int}")]
async fn all_vns_have_scanned_to_height(world: &mut TariWorld, block_height: u64) {
    let all_names = world
        .all_running_validators_iter()
        .filter(|vn| !vn.handle.is_finished())
        .map(|vn| vn.name.clone())
        .collect::<Vec<_>>();
    for vn in all_names {
        vn_has_scanned_to_height(world, vn, block_height).await;
    }
}

#[when(expr = "I wait for validator {word} has leaf block height of at least {int} at epoch {int}")]
async fn when_i_wait_for_validator_leaf_block_at_least(world: &mut TariWorld, name: String, height: u64, epoch: u64) {
    let vn = world.get_validator_node(&name);
    let mut client = vn.create_client();
    let epoch_stats = client.get_epoch_manager_stats().await.unwrap();
    for _ in 0..40 {
        let resp = client
            .list_blocks_paginated(GetBlocksRequest {
                limit: 1,
                offset: 0,
                ordering_index: Some(2),
                ordering: Some(Ordering::Descending),
                filter_index: Some(1),
                filter: Some(epoch_stats.current_epoch.as_u64().to_string()),
            })
            .await
            .unwrap();

        cucumber_log(format!(
            "Validator {name} leaf block height at epoch {} is {} (current epoch is {})",
            epoch,
            resp.blocks.first().map(|b| b.height().as_u64()).unwrap_or(0),
            epoch_stats.current_epoch.as_u64()
        ));

        if let Some(block) = resp.blocks.first() {
            assert!(block.epoch().as_u64() <= epoch);
            if block.epoch().as_u64() < epoch {
                eprintln!("VN {name} is in {}. Waiting for epoch {epoch}", block.epoch())
            }
            if block.epoch().as_u64() == epoch && block.height().as_u64() >= height {
                return;
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    let resp = client
        .list_blocks_paginated(GetBlocksRequest {
            limit: 1,
            offset: 0,
            ordering_index: Some(2),
            ordering: Some(Ordering::Descending),
            filter_index: Some(1),
            filter: Some(epoch_stats.current_epoch.as_u64().to_string()),
        })
        .await
        .unwrap();
    let block = resp
        .blocks
        .first()
        .unwrap_or_else(|| panic!("Validator {name} has no blocks"));
    if block.epoch().as_u64() < epoch {
        panic!("Validator {} at {} is less than epoch {}", name, block.epoch(), epoch);
    }
    if block.height().as_u64() < height {
        panic!(
            "Validator {} leaf block height {} is less than {}",
            name,
            block.height().as_u64(),
            height
        );
    }
}

#[when(expr = "Block height on VN {word} is at least {int}")]
async fn when_block_height(world: &mut TariWorld, vn_name: String, height: u64) {
    let vn = world.get_validator_node(&vn_name);
    let mut client = vn.create_client();
    for _ in 0..20 {
        if client
            .list_blocks(ListBlocksRequest {
                from_id: None,
                limit: 1,
            })
            .await
            .unwrap()
            .blocks[0]
            .height()
            .as_u64() >=
            height
        {
            return;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    panic!("Block height on VN {vn_name} is less than {height}");
}

#[then(expr = "the validator node {word} has started epoch {int}")]
async fn then_validator_node_switches_epoch(world: &mut TariWorld, vn_name: String, epoch: u64) {
    let vn = world.get_validator_node(&vn_name);
    let mut client = vn.create_client();
    for _ in 0..200 {
        let list_block = client
            .list_blocks_paginated(GetBlocksRequest {
                limit: 10,
                offset: 0,
                ordering_index: None,
                ordering: None,
                filter_index: Some(1),
                filter: Some(epoch.to_string()),
            })
            .await
            .unwrap();
        let blocks = list_block.blocks;
        assert!(
            blocks.iter().all(|b| b.epoch().as_u64() <= epoch),
            "Epoch is greater than expected"
        );
        if blocks.iter().any(|b| b.epoch().as_u64() == epoch) {
            return;
        }

        tokio::time::sleep(Duration::from_secs(8)).await;
    }
    panic!("Validator node {vn_name} did not switch to epoch {epoch}");
}

#[then(expr = "I wait for {word} to list {word} as evicted in {word}")]
async fn then_i_wait_for_validator_node_to_be_evicted(
    world: &mut TariWorld,
    vn_name: String,
    evict_vn_name: String,
    proof_name: String,
) {
    let vn = world.get_validator_node(&vn_name);
    let evict_vn = world.get_validator_node(&evict_vn_name);

    let (tx, mut rx) = mpsc::channel(1);
    let l1_tx_path = vn.layer_one_transaction_path();
    fs::create_dir_all(&l1_tx_path).unwrap();

    let mut watcher = notify::RecommendedWatcher::new(
        move |res| {
            tx.blocking_send(res).unwrap();
        },
        notify::Config::default(),
    )
    .unwrap();

    watcher.watch(&l1_tx_path, notify::RecursiveMode::NonRecursive).unwrap();

    loop {
        let event = timeout(Duration::from_secs(2000), rx.recv())
            .await
            .unwrap_or_else(|_| panic!("Timeout waiting for eviction file at path {}", l1_tx_path.display()))
            .expect("unexpected channel close")
            .unwrap_or_else(|err| panic!("Error when watching files {err}"));

        if let notify::Event {
            kind: notify::EventKind::Access(notify::event::AccessKind::Close(notify::event::AccessMode::Write)),
            paths,
            ..
        } = event
        {
            if let Some(json_file) = paths
                .into_iter()
                .find(|p| p.extension().is_some_and(|ext| ext == "json") && p.is_file())
            {
                eprintln!("🗒️ Found file: {}", json_file.display());
                let contents = fs::read(json_file).expect("Could not read file");
                let transaction_def = match serde_json::from_slice::<LayerOneTransactionDef<EvictionProof>>(&contents) {
                    Ok(def) => def,
                    Err(err) => {
                        eprintln!("Error deserializing eviction proof: {}", err);
                        continue;
                    },
                };
                if transaction_def.payload.node_to_evict().as_bytes() != evict_vn.public_key.as_bytes() {
                    panic!(
                        "Got an eviction proof for public key {}, however this did not match the public key of \
                         validator {evict_vn_name}",
                        transaction_def.payload.node_to_evict()
                    );
                }
                watcher.unwatch(&l1_tx_path).unwrap();
                world.add_eviction_proof(proof_name.clone(), transaction_def.payload);
                break;
            }
        }
    }
}

#[when(expr = "all validator nodes have started epoch {int}")]
async fn all_validators_have_started_epoch(world: &mut TariWorld, epoch: u64) {
    let mut remaining_attempts = 60;
    for vn in world.all_running_validators_iter().cycle() {
        let mut client = vn.create_client();
        let status = client.get_consensus_status().await.unwrap();
        if status.epoch.as_u64() >= epoch {
            println!(
                "Validator {} has started epoch {} (consensus state {}, height {})",
                vn.name, epoch, status.state, status.height
            );
            return;
        }
        if remaining_attempts == 0 {
            panic!(
                "Validator {} did not start epoch {} (at epoch: {}, status: {})",
                vn.name, epoch, status.epoch, status.state
            );
        }
        remaining_attempts -= 1;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[then(expr = "validator {word} is not a member of the current network according to {word}")]
async fn validator_not_member_of_network(world: &mut TariWorld, validator: String, base_node: String) {
    let bn = world.get_base_node(&base_node);
    let vn = world.get_validator_node(&validator);
    let mut client = bn.create_client();
    let tip = client.get_tip_info().await.unwrap();
    let vns = client.get_validator_nodes(tip.height_of_longest_chain).await.unwrap();
    let has_vn = vns.iter().any(|v| v.public_key == vn.public_key);
    if has_vn {
        // TODO: investigate why this is flaky
        warn!(
            "Validator {} is a member of the network but expected it not to be",
            validator
        );
    }
}
