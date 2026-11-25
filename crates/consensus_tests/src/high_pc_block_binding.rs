//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Tests for the rule that a proposal certificate becomes the high certificate only at the height, epoch and shard
//! group of the block it certifies.
//!
//! A certificate's id and block id are derived from the block's header hash and parent, so neither covers the
//! height, epoch or shard group the certificate claims. The stored block is the authority for those fields, and
//! `CertificateStore::update_highest` checks the certificate against it before writing HighPc, the leaf block or
//! the certificate record. A certificate of the zero block is checked against the epoch's genesis block.

use tari_consensus::traits::CertificateStore;
use tari_consensus_types::{HighPc, LeafBlock, ProposalCertificate};
use tari_engine_types::fees::ExhaustBurnRate;
use tari_ootle_common_types::{Epoch, NodeHeight, ProtocolVersion, ShardGroup, optional::Optional};
use tari_ootle_p2p::PeerAddress;
use tari_ootle_storage::{
    StateStore,
    consensus_models::{Block, BookkeepingModel},
};
use tari_ootle_transaction::Network;
use tari_sidechain::{FixedHash, QuorumDecision};
use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};

type TestStore = RocksDbStateStore<PeerAddress>;

const TEST_EPOCH: Epoch = Epoch(1);
const OTHER_EPOCH: Epoch = Epoch(2);

fn create_store() -> (TestStore, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let store = RocksDbStateStore::open(temp_dir.path(), DatabaseOptions::default()).unwrap();
    (store, temp_dir)
}

fn genesis_block() -> Block {
    Block::genesis(
        Network::LocalNet,
        ProtocolVersion::V0,
        TEST_EPOCH,
        FixedHash::zero(),
        ShardGroup::new(0, 127),
        FixedHash::zero(),
        None,
        ExhaustBurnRate::new(0),
    )
}

/// A certificate for `block` that claims `height` and `epoch`.
fn certificate_for(block: &Block, height: NodeHeight, epoch: Epoch) -> ProposalCertificate {
    certificate_claiming(block, height, epoch, block.shard_group())
}

/// A certificate for `block` that claims `height`, `epoch` and `shard_group`.
fn certificate_claiming(
    block: &Block,
    height: NodeHeight,
    epoch: Epoch,
    shard_group: ShardGroup,
) -> ProposalCertificate {
    let pc = ProposalCertificate::new(
        block.header().calculate_hash(),
        *block.parent(),
        height,
        epoch,
        shard_group,
        ProposalCertificate::genesis_signatures(),
        QuorumDecision::Accept,
    );
    assert_eq!(pc.calculate_block_id(), *block.id());
    pc
}

/// Stores the epoch genesis block with its own justify as the high certificate, as the worker does when an epoch
/// starts, along with the certificate for the genesis block itself.
fn store_with_genesis(store: &TestStore, genesis: &Block) -> ProposalCertificate {
    let genuine = certificate_for(genesis, genesis.height(), genesis.epoch());
    store
        .with_write_tx(|tx| {
            genesis.justify().save(tx)?;
            genesis.insert(tx)?;
            genesis.as_leaf().set(tx)?;
            genesis.justify().as_high_pc().set(tx)?;
            genuine.save(tx)
        })
        .unwrap();
    genuine
}

fn assert_nothing_installed(store: &TestStore, genesis: &Block, genuine: &ProposalCertificate) {
    store
        .with_read_tx(|tx| {
            let high_pc = HighPc::get(tx, TEST_EPOCH)?;
            assert_eq!(high_pc.height(), NodeHeight::zero());
            assert_eq!(*high_pc.id(), genesis.justify().calculate_id());

            let leaf = LeafBlock::get(tx, TEST_EPOCH)?;
            assert_eq!(leaf.height(), genesis.height());
            assert_eq!(*leaf.block_id(), *genesis.id());

            let stored = ProposalCertificate::get(tx, TEST_EPOCH, &genuine.calculate_id())?;
            assert_eq!(stored.height(), genesis.height());
            assert_eq!(stored.epoch(), genesis.epoch());
            Ok::<_, tari_ootle_storage::StorageError>(())
        })
        .unwrap();
}

#[test]
fn certificate_claiming_another_height_is_not_installed() {
    let (store, _tmp) = create_store();
    let genesis = genesis_block();
    let genuine = store_with_genesis(&store, &genesis);

    let relabelled = certificate_for(&genesis, NodeHeight(1_000_000), TEST_EPOCH);
    assert_eq!(relabelled.calculate_id(), genuine.calculate_id());

    store
        .with_write_tx(|tx| relabelled.update_highest(tx))
        .expect_err("a certificate whose height differs from its block's must not become the high certificate");

    assert_nothing_installed(&store, &genesis, &genuine);
}

#[test]
fn certificate_claiming_another_epoch_is_not_installed() {
    let (store, _tmp) = create_store();
    let genesis = genesis_block();
    let genuine = store_with_genesis(&store, &genesis);

    let relabelled = certificate_for(&genesis, genesis.height(), OTHER_EPOCH);
    store
        .with_write_tx(|tx| relabelled.update_highest(tx))
        .expect_err("a certificate whose epoch differs from its block's must not become the high certificate");

    assert_nothing_installed(&store, &genesis, &genuine);
    let high_pc = store
        .with_read_tx(|tx| HighPc::get(tx, OTHER_EPOCH).optional())
        .unwrap();
    assert!(high_pc.is_none());
}

#[test]
fn certificate_claiming_another_shard_group_is_not_installed() {
    let (store, _tmp) = create_store();
    let genesis = genesis_block();
    store.with_write_tx(|tx| genesis.insert(tx)).unwrap();

    let genuine = certificate_for(&genesis, genesis.height(), genesis.epoch());
    let relabelled = certificate_claiming(&genesis, genesis.height(), genesis.epoch(), ShardGroup::new(0, 63));
    assert_ne!(relabelled.shard_group(), genesis.shard_group());
    assert_eq!(relabelled.calculate_id(), genuine.calculate_id());

    store
        .with_write_tx(|tx| relabelled.update_highest(tx))
        .expect_err("a certificate whose shard group differs from its block's must not become the high certificate");

    store
        .with_read_tx(|tx| {
            assert!(HighPc::get(tx, TEST_EPOCH).optional()?.is_none());
            assert!(LeafBlock::get(tx, TEST_EPOCH).optional()?.is_none());
            let stored = ProposalCertificate::get(tx, TEST_EPOCH, &genuine.calculate_id()).optional()?;
            assert!(stored.is_none());
            Ok::<_, tari_ootle_storage::StorageError>(())
        })
        .unwrap();
}

#[test]
fn zero_block_certificate_claiming_another_shard_group_is_not_installed() {
    let (store, _tmp) = create_store();
    let genesis = genesis_block();
    store.with_write_tx(|tx| genesis.insert(tx)).unwrap();

    let relabelled = ProposalCertificate::genesis(genesis.epoch(), ShardGroup::new(0, 63));
    assert_ne!(relabelled.shard_group(), genesis.shard_group());
    assert_eq!(relabelled.calculate_id(), genesis.justify().calculate_id());

    store.with_write_tx(|tx| relabelled.update_highest(tx)).expect_err(
        "a zero-block certificate whose shard group differs from the genesis block's must not be installed",
    );

    store
        .with_read_tx(|tx| {
            assert!(HighPc::get(tx, TEST_EPOCH).optional()?.is_none());
            assert!(LeafBlock::get(tx, TEST_EPOCH).optional()?.is_none());
            let stored = ProposalCertificate::get(tx, TEST_EPOCH, &relabelled.calculate_id()).optional()?;
            assert!(stored.is_none());
            Ok::<_, tari_ootle_storage::StorageError>(())
        })
        .unwrap();
}

#[test]
fn zero_block_certificate_matching_the_genesis_block_is_installed() {
    let (store, _tmp) = create_store();
    let genesis = genesis_block();
    store.with_write_tx(|tx| genesis.insert(tx)).unwrap();

    let high_pc = store.with_write_tx(|tx| genesis.justify().update_highest(tx)).unwrap();

    assert_eq!(*high_pc.id(), genesis.justify().calculate_id());
    let leaf = store.with_read_tx(|tx| LeafBlock::get(tx, TEST_EPOCH)).unwrap();
    assert_eq!(leaf.shard_group(), genesis.shard_group());
}

#[test]
fn certificate_matching_its_block_is_installed() {
    let (store, _tmp) = create_store();
    let genesis = genesis_block();
    store.with_write_tx(|tx| genesis.insert(tx)).unwrap();

    let pc = certificate_for(&genesis, genesis.height(), genesis.epoch());
    let high_pc = store.with_write_tx(|tx| pc.update_highest(tx)).unwrap();

    assert_eq!(*high_pc.id(), pc.calculate_id());
    let leaf = store.with_read_tx(|tx| LeafBlock::get(tx, TEST_EPOCH)).unwrap();
    assert_eq!(*leaf.block_id(), *genesis.id());
    assert_eq!(leaf.height(), genesis.height());
}
