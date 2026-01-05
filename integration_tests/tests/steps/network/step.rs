//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use cucumber::{gherkin::Step, given};
use integration_tests::{
    base_node::spawn_base_node,
    indexer::spawn_indexer,
    miner::register_miner_process,
    validator_node::spawn_validator_node,
    wallet::spawn_minotari_wallet,
    wallet_daemon::spawn_wallet_daemon,
    wallet_daemon_client,
};
use tari_validator_node_client::types::AddPeerRequest;

use crate::{
    steps::{indexer, miner, network::spec::NetworkSpec, validator_node, wallet},
    TariWorld,
};

async fn create_network(world: &mut TariWorld, spec: NetworkSpec) {
    spawn_base_node(world, spec.base_node.name.clone()).await;
    spawn_minotari_wallet(world, spec.minotari_wallet.name.clone(), spec.base_node.name.clone()).await;
    register_miner_process(
        world,
        spec.miner.name.clone(),
        spec.base_node.name.clone(),
        spec.minotari_wallet.name.clone(),
    );
    spawn_indexer(world, spec.indexer.name.clone(), spec.base_node.name.clone()).await;

    for wallet_spec in &spec.walletds {
        spawn_wallet_daemon(world, wallet_spec.node.name.clone(), spec.indexer.name.clone()).await;
        if let Some(account) = &wallet_spec.with_account {
            let account =
                wallet_daemon_client::create_account(world, wallet_spec.node.name.clone(), account.clone()).await;
            integration_tests::cucumber_log!(format!(
                "Created initial account {} on wallet daemon {}",
                account, wallet_spec.node.name
            ));
        }
    }

    for vn_spec in &spec.validators {
        let vn = spawn_validator_node(
            world,
            vn_spec.node.name.clone(),
            spec.base_node.name.clone(),
            vn_spec.fee_claim_account.as_deref(),
        )
        .await;

        world
            .get_indexer(&spec.indexer.name)
            .add_peer(vn.public_key, vn.p2p_port)
            .await;
        world.validator_nodes.insert(vn_spec.node.name.clone(), vn);
    }

    // Connect validators to each other
    for (i, vn) in world.validator_nodes.values().enumerate() {
        for (j, vn_inner) in world.validator_nodes.values().enumerate() {
            if i == j {
                continue;
            }
            let mut client = vn.create_client();
            client
                .add_peer(AddPeerRequest {
                    public_key: vn_inner.public_key,
                    addresses: vn_inner.get_addresses(),
                    wait_for_dial: false,
                })
                .await
                .unwrap();
        }
    }

    let num_blocks = 10 + spec.validators.len() as u64;
    miner::miner_mines_new_blocks(world, spec.miner.name.clone(), num_blocks).await;
    integration_tests::cucumber_log!(format!("Mined {num_blocks} blocks"));
    wallet::check_balance(world, spec.minotari_wallet.name.clone(), 20, "T".to_string()).await;

    for vn_spec in &spec.validators {
        validator_node::send_vn_registration(world, vn_spec.name().to_string(), spec.minotari_wallet.name.clone())
            .await;
        integration_tests::cucumber_log!(format!("Validator node {} sent registration", vn_spec.name()));
    }

    let scan_height = 20 + num_blocks - world.consensus_constants.base_layer_confirmations;
    miner::miner_mines_new_blocks(world, spec.miner.name.clone(), 20).await;
    integration_tests::cucumber_log!("Mined 20 blocks");
    indexer::indexer_has_scanned_to_at_least_height(world, spec.indexer.name.clone(), scan_height).await;
    integration_tests::cucumber_log!(format!("Indexer has scanned up to or past height {}", scan_height));

    for vn_spec in &spec.validators {
        validator_node::assert_vn_is_registered(world, vn_spec.name().to_string()).await;
        integration_tests::cucumber_log!(format!("Validator node {} is registered", vn_spec.name()));
        world
            .get_validator_node(vn_spec.name())
            .wait_for_consensus_to_start()
            .await;
        integration_tests::cucumber_log!(format!("Validator node {} consensus started", vn_spec.name()));
    }
}

#[given(expr = "a network with spec")]
async fn start_a_network_with_spec(world: &mut TariWorld, step: &Step) {
    let mut spec = NetworkSpec::default();

    if let Some(spec_toml) = step.docstring.as_ref() {
        spec = serde_yaml::from_str::<NetworkSpec>(spec_toml).expect("Failed to parse network spec");
    }

    create_network(world, spec).await;
}

#[given(expr = "a network with registered validator {word} and wallet daemon {word}")]
async fn start_a_network(world: &mut TariWorld, vn_name: String, walletd_name: String) {
    let mut spec = NetworkSpec::default();
    spec.validators[0].node.name = vn_name.clone();
    spec.walletds[0].node.name = walletd_name.clone();

    create_network(world, spec).await;
}
