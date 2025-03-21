//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::{Epoch, NodeHeight, NumPreshards, ShardGroup};
use tari_dan_storage::{
    consensus_models::{QuorumCertificate, QuorumDecision},
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};

use crate::helper::{assert_eq_debug, create_random_block_id, create_rocksdb, create_sqlite};

#[test]
fn quorum_certificates_sqlite() {
    let db = create_sqlite();
    db.foreign_keys_off().unwrap();
    quorum_certificates_operations(db);
}

#[test]
fn quorum_certificates_rocksdb() {
    let (db, _tmp) = create_rocksdb();
    quorum_certificates_operations(db);
}

fn quorum_certificates_operations(db: impl StateStore) {
    let mut tx = db.create_write_tx().unwrap();

    let epoch = Epoch::zero();
    let shard_group = ShardGroup::all_shards(NumPreshards::P4);
    let genesis_qc = QuorumCertificate::genesis(epoch, shard_group);

    let block_id_1 = create_random_block_id();
    let qc1 = QuorumCertificate::new(
        *genesis_qc.header_hash(),
        block_id_1,
        genesis_qc.block_height() + NodeHeight(1),
        epoch,
        shard_group,
        vec![],
        vec![],
        QuorumDecision::Accept,
    );

    // insert both QCs in database
    tx.quorum_certificates_insert(&genesis_qc).unwrap();
    tx.quorum_certificates_insert(&qc1).unwrap();

    // quorum_certificates_get
    let res = tx.quorum_certificates_get(genesis_qc.id()).unwrap();
    assert_eq_debug(&res, &genesis_qc);
    let res = tx.quorum_certificates_get(qc1.id()).unwrap();
    assert_eq_debug(&res, &qc1);

    // quorum_certificates_get_all
    let qc_ids = vec![genesis_qc.id(), qc1.id()];
    let res = tx.quorum_certificates_get_all(qc_ids).unwrap();
    assert_eq!(res.len(), 2);

    // quorum_certificates_get_by_block_id
    let res = tx.quorum_certificates_get_by_block_id(genesis_qc.block_id()).unwrap();
    assert_eq_debug(&res, &genesis_qc);
    let res = tx.quorum_certificates_get_by_block_id(qc1.block_id()).unwrap();
    assert_eq_debug(&res, &qc1);

    tx.rollback().unwrap();
}
