//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus::hotstuff::eviction_proof::convert_block_to_sidechain_block_header;
use tari_dan_storage::consensus_models::Block;

use crate::support::load_json_fixture;

#[test]
fn it_produces_a_summarized_header_that_hashes_to_the_original() {
    let block = load_json_fixture::<Block>("block.json");
    let sidechain_block = convert_block_to_sidechain_block_header(block.header());
    assert_eq!(sidechain_block.extra_data_hash, block.header().create_extra_data_hash());
    assert_eq!(
        sidechain_block.base_layer_block_hash,
        *block.header().base_layer_block_hash()
    );
    assert_eq!(
        sidechain_block.base_layer_block_height,
        block.header().base_layer_block_height()
    );
    assert_eq!(sidechain_block.timestamp, block.header().timestamp());
    assert_eq!(
        sidechain_block.signature,
        block.header().signature().expect("checked by caller").clone()
    );
    assert_eq!(
        sidechain_block.foreign_indexes_hash,
        block.header().create_foreign_indexes_hash()
    );
    assert_eq!(sidechain_block.is_dummy, block.header().is_dummy());
    assert_eq!(
        sidechain_block.command_merkle_root,
        *block.header().command_merkle_root()
    );
    assert_eq!(sidechain_block.state_merkle_root, *block.header().state_merkle_root());
    assert_eq!(sidechain_block.total_leader_fee, block.header().total_leader_fee());
    assert_eq!(sidechain_block.proposed_by, block.header().proposed_by().clone());
    assert_eq!(
        sidechain_block.shard_group.start,
        block.header().shard_group().start().as_u32()
    );
    assert_eq!(
        sidechain_block.shard_group.end_inclusive,
        block.header().shard_group().end().as_u32()
    );
    assert_eq!(sidechain_block.epoch, block.header().epoch().as_u64());
    assert_eq!(sidechain_block.height, block.header().height().as_u64());
    assert_eq!(sidechain_block.justify_id, *block.header().justify_id().hash());
    assert_eq!(sidechain_block.parent_id, *block.header().parent().hash());
    assert_eq!(sidechain_block.network, block.header().network().as_byte());

    // Finally check the hash matches
    assert_eq!(sidechain_block.calculate_hash(), block.header().calculate_hash());
    assert_eq!(
        sidechain_block.calculate_block_id(),
        *block.header().calculate_id().hash()
    );
}
