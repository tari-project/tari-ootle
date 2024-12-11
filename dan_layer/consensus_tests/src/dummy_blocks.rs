//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common::configuration::Network;
use tari_common_types::types::{FixedHash, PrivateKey, PublicKey};
use tari_consensus::hotstuff::{
    calculate_dummy_blocks,
    calculate_dummy_blocks_from_justify,
    calculate_last_dummy_block,
};
use tari_crypto::{keys::PublicKey as _, tari_utilities::ByteArray};
use tari_dan_common_types::{committee::Committee, DerivableFromPublicKey, Epoch, NodeHeight, PeerAddress, ShardGroup};
use tari_dan_storage::consensus_models::Block;

use crate::support::{load_fixture, RoundRobinLeaderStrategy};

#[test]
fn dummy_blocks() {
    let genesis = Block::genesis(
        Network::LocalNet,
        Epoch(1),
        ShardGroup::new(0, 127),
        FixedHash::zero(),
        None,
    );
    let committee = (0u8..2)
        .map(public_key_from_seed)
        .map(|pk| (PeerAddress::derive_from_public_key(&pk), pk))
        .collect();

    let dummy = calculate_dummy_blocks(
        NodeHeight(0),
        NodeHeight(30),
        Network::LocalNet,
        Epoch(1),
        ShardGroup::new(0, 127),
        *genesis.id(),
        genesis.justify(),
        genesis.id(),
        FixedHash::zero(),
        &RoundRobinLeaderStrategy,
        &committee,
        genesis.timestamp(),
        0,
        FixedHash::zero(),
    );
    let last = calculate_last_dummy_block(
        NodeHeight(0),
        NodeHeight(30),
        Network::LocalNet,
        Epoch(1),
        ShardGroup::new(0, 127),
        *genesis.id(),
        genesis.justify(),
        FixedHash::zero(),
        &RoundRobinLeaderStrategy,
        &committee,
        genesis.timestamp(),
        0,
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

fn public_key_from_seed(seed: u8) -> PublicKey {
    PublicKey::from_secret_key(&PrivateKey::from_canonical_bytes(&[seed; 32]).unwrap())
}

#[test]
fn last_matches_generated_using_real_data() {
    let candidate = load_fixture::<Block>("block_with_dummies.json");

    let committee = load_fixture::<serde_json::Value>("committee.json");
    let committee: Vec<(PeerAddress, PublicKey)> = serde_json::from_value(committee["members"].clone()).unwrap();
    let committee = Committee::new(committee);

    let justify = Block::genesis(
        Network::LocalNet,
        candidate.epoch(),
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
        justify.base_layer_block_height(),
        *justify.base_layer_block_hash(),
    )
    .expect("last dummy block");

    assert_eq!(dummy.last().unwrap().id(), last.block_id());
}
