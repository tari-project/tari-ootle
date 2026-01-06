//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use cucumber::{gherkin::Step, given, then, when};
use tari_base_node_client::BaseNodeClient;

use crate::{mine_blocks, register_miner_process, TariWorld};

#[given(expr = "a miner {word} connected to base node {word} and wallet {word}")]
async fn create_miner(world: &mut TariWorld, step: &Step, miner_name: String, bn_name: String, wallet_name: String) {
    integration_tests::cucumber_log!("==== Step: {}", step.value);
    register_miner_process(world, miner_name, bn_name, wallet_name);
}

#[when(expr = "miner {word} mines {int} new blocks")]
pub async fn miner_mines_new_blocks(world: &mut TariWorld, step: &Step, miner_name: String, num_blocks: u64) {
    integration_tests::cucumber_log!("==== Step: {}", step.value);
    let bn = world
        .base_nodes
        .values()
        .next()
        .expect("Cannot mine because there are no base nodes");
    let mut client = bn.create_client();
    let start_tip = client.get_tip_info().await.unwrap().height_of_longest_chain;

    mine_blocks(world, miner_name, num_blocks).await;

    wait_for_all_base_nodes_to_reach_tip(world, start_tip + num_blocks).await;
}

#[then(expr = "miner {word} mines to the next epoch")]
pub async fn miner_mines_to_next_epoch(world: &mut TariWorld, step: &Step, miner_name: String) {
    integration_tests::cucumber_log!("==== Step: {}", step.value);
    let bn = world
        .base_nodes
        .values()
        .next()
        .expect("Cannot mine because there are no base nodes");
    let mut client = bn.create_client();

    let start_tip = client.get_tip_info().await.unwrap().height_of_longest_chain;

    let consensus_constants = client.get_consensus_constants(start_tip).await.unwrap();

    let epoch = start_tip / consensus_constants.epoch_length;
    let next_epoch_start = (epoch + 1) * consensus_constants.epoch_length;

    let num_blocks = next_epoch_start - start_tip + world.consensus_constants.base_layer_confirmations;

    mine_blocks(world, miner_name, num_blocks).await;

    wait_for_all_base_nodes_to_reach_tip(world, start_tip + num_blocks).await;
}

async fn wait_for_all_base_nodes_to_reach_tip(world: &mut TariWorld, height: u64) {
    // wait for all tips to be the new height
    for bn in world.base_nodes.values() {
        let mut client = bn.create_client();
        let mut tip = client.get_tip_info().await.unwrap();
        let mut iter_count = 0;
        while tip.height_of_longest_chain < height {
            tip = client.get_tip_info().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if iter_count > 100 {
                panic!("Timed out waiting for tip height to reach {}", height);
            }
            iter_count += 1;
        }
        eprintln!(
            "Base node {} reached tip height {}",
            bn.name, tip.height_of_longest_chain
        );
    }
}
