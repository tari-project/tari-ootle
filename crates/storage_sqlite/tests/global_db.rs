//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::{Connection, SqliteConnection};
use ootle_byte_type::ToByteType;
use tari_common_types::types::FixedHash;
use tari_crypto::{keys::PublicKey, ristretto::RistrettoPublicKey};
use tari_ootle_common_types::{Epoch, NumPreshards, ShardGroup, SubstateAddress, VotePower};
use tari_ootle_p2p::PeerAddress;
use tari_ootle_storage::global::{BlockHeaderModel, GlobalDb, ValidatorNodeDb};
use tari_ootle_storage_sqlite::global::SqliteGlobalDbAdapter;
use tari_utilities::ByteArray;

fn create_db() -> GlobalDb<SqliteGlobalDbAdapter<PeerAddress>> {
    let conn = SqliteConnection::establish(":memory:").unwrap();
    let db = GlobalDb::new(SqliteGlobalDbAdapter::new(conn));
    db.adapter().migrate().unwrap();
    db
}

fn new_public_key() -> RistrettoPublicKey {
    RistrettoPublicKey::random_keypair(&mut rand::rng()).1
}

fn derived_substate_address(public_key: &RistrettoPublicKey) -> SubstateAddress {
    let hash = FixedHash::try_from(public_key.as_bytes()).unwrap();
    let mut arr = [0u8; SubstateAddress::LENGTH];
    arr[..hash.as_bytes().len()].copy_from_slice(hash.as_bytes());
    SubstateAddress::from_array(arr)
}

fn insert_vns(
    validator_nodes: &mut ValidatorNodeDb<'_, '_, SqliteGlobalDbAdapter<PeerAddress>>,
    num: usize,
    epoch: Epoch,
) {
    for _ in 0..num {
        let pk = new_public_key();
        insert_vn_with_public_key(validator_nodes, pk.clone(), epoch);
        set_committee_shard_group(validator_nodes, &pk, ShardGroup::all_shards(NumPreshards::P256), epoch);
    }
}

fn insert_vn_with_public_key(
    validator_nodes: &mut ValidatorNodeDb<'_, '_, SqliteGlobalDbAdapter<PeerAddress>>,
    public_key: RistrettoPublicKey,
    start_epoch: Epoch,
) {
    validator_nodes
        .insert_validator_node(
            public_key.clone().into(),
            public_key.to_byte_type(),
            derived_substate_address(&public_key),
            start_epoch,
            public_key.to_byte_type(),
            VotePower::of(1),
        )
        .unwrap()
}

fn set_committee_shard_group(
    validator_nodes: &mut ValidatorNodeDb<'_, '_, SqliteGlobalDbAdapter<PeerAddress>>,
    public_key: &RistrettoPublicKey,
    shard_group: ShardGroup,
    epoch: Epoch,
) {
    validator_nodes
        .set_committee_shard(derived_substate_address(public_key), shard_group, epoch)
        .unwrap();
}

#[test]
fn insert_and_get_within_epoch() {
    let db = create_db();
    let mut tx = db.create_transaction().unwrap();
    let mut validator_nodes = db.validator_nodes(&mut tx);
    insert_vns(&mut validator_nodes, 3, Epoch(0));
    insert_vns(&mut validator_nodes, 2, Epoch(1));
    let vns = validator_nodes.get_all_registered_within_start_epoch(Epoch(0)).unwrap();
    assert_eq!(vns.len(), 3);
}

#[test]
fn change_committee_shard_group() {
    let db = create_db();
    let mut tx = db.create_transaction().unwrap();
    let mut validator_nodes = db.validator_nodes(&mut tx);
    let pk = new_public_key();
    insert_vn_with_public_key(&mut validator_nodes, pk.clone(), Epoch(0));
    set_committee_shard_group(&mut validator_nodes, &pk, ShardGroup::new(1, 2), Epoch(0));
    let count = validator_nodes.count(Epoch(0)).unwrap();
    assert_eq!(count, 1);
    set_committee_shard_group(&mut validator_nodes, &pk, ShardGroup::new(3, 4), Epoch(1));
    set_committee_shard_group(&mut validator_nodes, &pk, ShardGroup::new(7, 8), Epoch(2));
    set_committee_shard_group(&mut validator_nodes, &pk, ShardGroup::new(4, 5), Epoch(3));
    let pk2 = new_public_key();
    insert_vn_with_public_key(&mut validator_nodes, pk2.clone(), Epoch(3));
    set_committee_shard_group(&mut validator_nodes, &pk2, ShardGroup::new(4, 5), Epoch(3));
    let count = validator_nodes.count(Epoch(0)).unwrap();
    assert_eq!(count, 1);
    let count = validator_nodes.count(Epoch(3)).unwrap();
    assert_eq!(count, 2);
    let vns = validator_nodes
        .get_committee_for_shard_group(Epoch(3), ShardGroup::new(4, 5), 100)
        .unwrap();
    assert_eq!(vns.len(), 2);
}

#[test]
fn block_header_insert_is_idempotent() {
    // On reorg detection the base-layer scanner rewinds to the fork point and re-scans, which can
    // re-insert already-seen (block_hash, epoch) rows. These must be swallowed rather than erroring
    // out the scan.
    let db = create_db();
    let mut tx = db.create_transaction().unwrap();
    let mut headers = db.block_headers(&mut tx);
    let model = BlockHeaderModel {
        epoch: Epoch(1),
        height: 100,
        block_hash: FixedHash::from([1u8; 32]),
        kernel_merkle_root: FixedHash::from([2u8; 32]),
        validator_node_merkle_root: FixedHash::from([3u8; 32]),
    };
    headers.insert(model.clone()).unwrap();
    // Second insert of the same (block_hash, epoch) must succeed without error.
    headers.insert(model).unwrap();

    // A different epoch with the same hash should also succeed (the unique index is on the pair).
    let other_epoch = BlockHeaderModel {
        epoch: Epoch(2),
        height: 200,
        block_hash: FixedHash::from([1u8; 32]),
        kernel_merkle_root: FixedHash::from([4u8; 32]),
        validator_node_merkle_root: FixedHash::from([5u8; 32]),
    };
    headers.insert(other_epoch).unwrap();
}

#[test]
fn delete_block_headers_above_removes_higher_headers() {
    // On reorg recovery the scanner deletes every header above the fork point so the canonical
    // headers can be re-scanned in their place (see base_layer/oracle.rs::handle_reorg).
    let db = create_db();
    let mut tx = db.create_transaction().unwrap();
    let mut headers = db.block_headers(&mut tx);
    for height in [100u64, 101, 102, 103] {
        headers
            .insert(BlockHeaderModel {
                epoch: Epoch(1),
                height,
                block_hash: FixedHash::from([height as u8; 32]),
                kernel_merkle_root: FixedHash::from([2u8; 32]),
                validator_node_merkle_root: FixedHash::from([3u8; 32]),
            })
            .unwrap();
    }

    // Heights 102 and 103 sit above the fork point at 101 and must be removed.
    assert_eq!(headers.delete_above(101).unwrap(), 2);
    // The fork-point block and everything below it are retained.
    assert_eq!(headers.delete_above(0).unwrap(), 2);
    // Nothing left to delete.
    assert_eq!(headers.delete_above(0).unwrap(), 0);
}
