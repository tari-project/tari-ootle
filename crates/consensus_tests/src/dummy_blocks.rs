//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use tari_common_types::types::FixedHash;
use tari_consensus::hotstuff::{
    calculate_dummy_blocks,
    calculate_dummy_blocks_from_justify,
    calculate_last_dummy_block,
};
use tari_consensus_types::{BlockId, ShardGroupAccumulatedData};
use tari_crypto::tari_utilities::hex::Hex;
use tari_ootle_common_types::{
    DerivableFromPublicKey,
    Epoch,
    Network,
    NodeHeight,
    NumPreshards,
    ShardGroup,
    VotePower,
    committee::{Committee, CommitteeMember},
    crypto::create_key_pair_from_seed,
};
use tari_ootle_p2p::PeerAddress;
use tari_ootle_storage::consensus_models::Block;

use crate::support::{RoundRobinLeaderStrategy, load_json_fixture};

#[test]
fn dummy_blocks() {
    let shard_group = ShardGroup::new(1, 127);
    let genesis = Block::genesis(
        Network::LocalNet,
        Epoch(1),
        FixedHash::zero(),
        shard_group,
        FixedHash::zero(),
        None,
    );
    let committee = (0u8..2)
        .map(create_key_pair_from_seed)
        .map(|(_, pk)| CommitteeMember {
            address: PeerAddress::derive_from_public_key(&pk),
            public_key: pk.to_byte_type(),
            vote_power: VotePower::of(1),
        })
        .collect();

    let dummy = calculate_dummy_blocks(
        NodeHeight(0),
        NodeHeight(30),
        Network::LocalNet,
        Epoch(1),
        shard_group,
        *genesis.id(),
        genesis.justify(),
        genesis.id(),
        FixedHash::zero(),
        &RoundRobinLeaderStrategy,
        &committee,
        genesis.timestamp(),
        ShardGroupAccumulatedData::default(),
        FixedHash::zero(),
    );
    let last = calculate_last_dummy_block(
        NodeHeight(0),
        NodeHeight(30),
        Network::LocalNet,
        Epoch(1),
        shard_group,
        *genesis.id(),
        genesis.justify(),
        FixedHash::zero(),
        &RoundRobinLeaderStrategy,
        &committee,
        genesis.timestamp(),
        ShardGroupAccumulatedData::default(),
        FixedHash::zero(),
    )
    .expect("last dummy block");
    assert_eq!(dummy[0].parent(), genesis.id());
    for i in 1..dummy.len() {
        assert_eq!(dummy[i].parent(), dummy[i - 1].id());
    }
    assert_eq!(dummy.last().unwrap().id(), last.block_id());
    assert_eq!(dummy.len(), 29);
}

#[test]
fn last_matches_generated_using_real_data() {
    let candidate = load_json_fixture::<Block>("block_with_dummies.json");

    let committee = load_json_fixture::<serde_json::Value>("committee.json");
    let committee: Vec<CommitteeMember<PeerAddress>> = serde_json::from_value(committee["members"].clone()).unwrap();
    let committee = Committee::new(committee);

    let justify = Block::genesis(
        Network::LocalNet,
        candidate.epoch(),
        FixedHash::zero(),
        candidate.shard_group(),
        FixedHash::zero(),
        None,
    );

    let dummy = calculate_dummy_blocks_from_justify(&candidate, &justify, &RoundRobinLeaderStrategy, &committee);

    let last = calculate_last_dummy_block(
        justify.height(),
        candidate.height(),
        Network::LocalNet,
        justify.epoch(),
        justify.shard_group(),
        *justify.id(),
        justify.justify(),
        *justify.state_merkle_root(),
        &RoundRobinLeaderStrategy,
        &committee,
        justify.timestamp(),
        ShardGroupAccumulatedData::default(),
        *justify.epoch_hash(),
    )
    .expect("last dummy block");

    assert_eq!(dummy.last().unwrap().id(), last.block_id());
}

/// Regression test: when the QC justifies the zero block (no blocks committed yet in the epoch),
/// the proposer must use the epoch genesis block (which has a state_merkle_root and
/// epoch_hash set from the previous epoch checkpoint) rather than the global zero block (all-zero fields). Using the
/// wrong block causes every dummy block ID to diverge, making all proposals permanently invalid.
#[test]
fn dummy_blocks_from_epoch_genesis_vs_zero_block() {
    let shard_group = ShardGroup::all_shards(NumPreshards::P256);
    let non_zero_state_root =
        FixedHash::from_hex("613a7a1b6b83edb2d49c4d740b8b0e7e4ee226b453b004b04d4812dbc51306d9").unwrap();
    let non_zero_epoch_hash =
        FixedHash::from_hex("7da2c68f183ed4a96109e5ebeb18f7e26082f928b9f9879685f90c0bee041451").unwrap();

    // The epoch genesis has real state carried over from the previous epoch
    let epoch_genesis = Block::genesis(
        Network::LocalNet,
        Epoch(100),
        non_zero_epoch_hash,
        shard_group,
        non_zero_state_root,
        None,
    );

    // The zero block has all-zero fields - this is what the buggy proposer was using
    let zero_block = Block::zero_block(Network::LocalNet, NumPreshards::P256);

    let committee: Committee<PeerAddress> = (0u8..4)
        .map(create_key_pair_from_seed)
        .map(|(_, pk)| CommitteeMember {
            address: PeerAddress::derive_from_public_key(&pk),
            public_key: pk.to_byte_type(),
            vote_power: VotePower::of(1),
        })
        .collect();

    let candidate_height = NodeHeight(50);
    let qc = epoch_genesis.justify();

    // Simulate the VALIDATOR path: uses epoch genesis (correct)
    let validator_dummies = calculate_dummy_blocks(
        epoch_genesis.height(),
        candidate_height,
        Network::LocalNet,
        Epoch(100),
        shard_group,
        *epoch_genesis.id(),
        qc,
        &BlockId::zero(), // unused expected_parent - we'll check the last dummy directly
        *epoch_genesis.state_merkle_root(),
        &RoundRobinLeaderStrategy,
        &committee,
        epoch_genesis.timestamp(),
        *epoch_genesis.header().accumulated_data(),
        *epoch_genesis.epoch_hash(),
    );

    // Simulate the BUGGY PROPOSER path: uses zero block instead of epoch genesis
    let buggy_proposer_last = calculate_last_dummy_block(
        zero_block.height(),
        candidate_height,
        Network::LocalNet,
        Epoch(100),
        zero_block.shard_group(),
        *zero_block.id(),
        qc,
        *zero_block.state_merkle_root(),
        &RoundRobinLeaderStrategy,
        &committee,
        zero_block.timestamp(),
        *zero_block.header().accumulated_data(),
        *zero_block.epoch_hash(),
    )
    .unwrap();

    // The buggy path produces different dummy block IDs - this was the cause of permanent
    // proposal rejection after leader failure at the start of a new epoch
    assert_ne!(
        validator_dummies.last().unwrap().id(),
        &buggy_proposer_last.block_id,
        "zero block and epoch genesis should produce different dummy chains"
    );

    // Simulate the FIXED PROPOSER path: uses epoch genesis (matches validator)
    let fixed_proposer_last = calculate_last_dummy_block(
        epoch_genesis.height(),
        candidate_height,
        Network::LocalNet,
        Epoch(100),
        epoch_genesis.shard_group(),
        *epoch_genesis.id(),
        qc,
        *epoch_genesis.state_merkle_root(),
        &RoundRobinLeaderStrategy,
        &committee,
        epoch_genesis.timestamp(),
        *epoch_genesis.header().accumulated_data(),
        *epoch_genesis.epoch_hash(),
    )
    .unwrap();

    // The fixed path matches the validator
    assert_eq!(
        validator_dummies.last().unwrap().id(),
        &fixed_proposer_last.block_id,
        "epoch genesis should produce matching dummy chains between proposer and validator"
    );
}
