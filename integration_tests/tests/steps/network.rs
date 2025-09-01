//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use cucumber::given;
use integration_tests::{
    base_node::spawn_base_node,
    indexer::spawn_indexer,
    miner::register_miner_process,
    util::cucumber_log,
    validator_node::spawn_validator_node,
    wallet::spawn_wallet,
    wallet_daemon::spawn_wallet_daemon,
};

use crate::{
    steps::{indexer, miner, validator_node, wallet},
    TariWorld,
};

#[given(expr = "a network with registered validator {word} and wallet daemon {word}")]
async fn start_a_network(world: &mut TariWorld, vn_name: String, walletd_name: String) {
    const BASE_NODE_NAME: &str = "NETWORK_BASE_NODE";
    const CONSOLE_WALLET_NAME: &str = "NETWORK_CONSOLE_WALLET";
    const MINER_NAME: &str = "NETWORK_MINER";
    const INDEXER_NAME: &str = "NETWORK_INDEXER";

    spawn_base_node(world, BASE_NODE_NAME.to_string()).await;
    cucumber_log("Base node started");
    spawn_wallet(world, CONSOLE_WALLET_NAME.to_string(), BASE_NODE_NAME.to_string()).await;
    cucumber_log("Console wallet started");
    register_miner_process(
        world,
        MINER_NAME.to_string(),
        BASE_NODE_NAME.to_string(),
        CONSOLE_WALLET_NAME.to_string(),
    );
    cucumber_log("Miner started");
    spawn_indexer(world, INDEXER_NAME.to_string(), BASE_NODE_NAME.to_string()).await;
    cucumber_log("Indexer started");
    spawn_wallet_daemon(world, walletd_name.clone(), INDEXER_NAME.to_string()).await;
    cucumber_log("Wallet daemon started");
    let vn = spawn_validator_node(
        world,
        vn_name.clone(),
        BASE_NODE_NAME.to_string(),
        walletd_name,
        format!("{}_claim_fee", vn_name),
    )
    .await;
    cucumber_log("Validator node started");

    world
        .get_indexer(INDEXER_NAME)
        .add_peer(vn.public_key, vn.p2p_port)
        .await;
    world.validator_nodes.insert(vn_name.clone(), vn);

    miner::miner_mines_new_blocks(world, MINER_NAME.to_string(), 10).await;
    cucumber_log("Mined 10 blocks");
    wallet::check_balance(world, CONSOLE_WALLET_NAME.to_string(), 20, "T".to_string()).await;
    cucumber_log("Console wallet has balance");
    validator_node::send_vn_registration(world, vn_name.clone(), CONSOLE_WALLET_NAME.to_string()).await;
    cucumber_log("Validator node sent registration");
    miner::miner_mines_new_blocks(world, MINER_NAME.to_string(), 20).await;
    cucumber_log("Mined 26 blocks");
    indexer::indexer_has_scanned_to_at_least_height(world, INDEXER_NAME.to_string(), 20).await;
    cucumber_log("Indexer has scanned up to or past height 26");
    validator_node::assert_vn_is_registered(world, vn_name.clone()).await;
    cucumber_log("Validator node is registered");
    world.get_validator_node(&vn_name).wait_for_consensus_to_start().await;
    cucumber_log("Validator node consensus started");
}
