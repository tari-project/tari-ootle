//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use indexmap::IndexMap;
use tari_dan_common_types::{Epoch, NodeHeight, NumPreshards, ShardGroup};
use tari_dan_storage::{
    consensus_models::{
        Block,
        BlockId,
        BlockPledge,
        EpochCheckpoint,
        ForeignParkedProposal,
        ForeignProposal,
        ForeignProposalStatus,
        HighQc,
        LastExecuted,
        LastSentVote,
        LastVoted,
        LeafBlock,
        LockedBlock,
        QcId,
        QuorumCertificate,
        QuorumDecision,
        ValidatorSignature,
    },
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};
use tari_state_tree::TreeHash;
use tari_template_lib::prelude::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

use crate::helper::{assert_eq_debug, create_rocksdb, create_sqlite};

#[test]
fn miscellaneous_sqlite() {
    let db = create_sqlite();
    db.foreign_keys_off().unwrap();
    miscellaneous_operations(db);
}

#[test]
fn miscellaneous_rocksdb() {
    let (db, _tmp) = create_rocksdb();
    miscellaneous_operations(db);
}

#[allow(clippy::too_many_lines)]
fn miscellaneous_operations(db: impl StateStore) {
    let mut tx = db.create_write_tx().unwrap();

    // last voted
    let mut last_voted = LastVoted {
        block_id: BlockId::zero(),
        height: NodeHeight(123),
        epoch: Epoch::zero(),
    };
    tx.last_voted_set(&last_voted).unwrap();
    let res = tx.last_voted_get().unwrap();
    assert_eq_debug(&res, &last_voted);

    last_voted.epoch += Epoch(1);

    tx.last_voted_set(&last_voted).unwrap();
    let res = tx.last_voted_get().unwrap();
    assert_eq_debug(&res, &last_voted);

    // last sent vote
    let mut last_sent_vote = LastSentVote {
        block_id: BlockId::zero(),
        epoch: Epoch::zero(),
        block_height: NodeHeight(123),
        decision: QuorumDecision::Accept,
        signature: ValidatorSignature::new(RistrettoPublicKeyBytes::default(), SchnorrSignatureBytes::zero()),
    };
    tx.last_sent_vote_set(&last_sent_vote).unwrap();
    let res = tx.last_sent_vote_get().unwrap();
    assert_eq_debug(&res, &last_sent_vote);

    last_sent_vote.epoch += Epoch(1);

    tx.last_sent_vote_set(&last_sent_vote).unwrap();
    let res = tx.last_sent_vote_get().unwrap();
    assert_eq_debug(&res, &last_sent_vote);

    // last executed
    let mut last_exec = LastExecuted {
        block_id: BlockId::zero(),
        height: NodeHeight(123),
        epoch: Epoch::zero(),
    };
    tx.last_executed_set(&last_exec).unwrap();
    let res = tx.last_executed_get().unwrap();
    assert_eq_debug(&res, &last_exec);

    last_exec.epoch += Epoch(1);

    tx.last_executed_set(&last_exec).unwrap();
    let res = tx.last_executed_get().unwrap();
    assert_eq_debug(&res, &last_exec);

    // last executed
    let mut last_exec = LastExecuted {
        block_id: BlockId::zero(),
        height: NodeHeight(123),
        epoch: Epoch::zero(),
    };
    tx.last_executed_set(&last_exec).unwrap();
    let res = tx.last_executed_get().unwrap();
    assert_eq_debug(&res, &last_exec);

    last_exec.epoch += Epoch(1);

    tx.last_executed_set(&last_exec).unwrap();
    let res = tx.last_executed_get().unwrap();
    assert_eq_debug(&res, &last_exec);

    // locked block
    let epoch = Epoch::zero();
    let mut locked_block = LockedBlock {
        block_id: BlockId::zero(),
        height: NodeHeight(123),
        epoch,
    };
    tx.locked_block_set(&locked_block).unwrap();
    let res = tx.locked_block_get(epoch).unwrap();
    assert_eq_debug(&res, &locked_block);

    locked_block.height += NodeHeight(1);

    tx.locked_block_set(&locked_block).unwrap();
    let res = tx.locked_block_get(epoch).unwrap();
    assert_eq_debug(&res, &locked_block);

    // leaf block
    let epoch = Epoch::zero();
    let mut leaf_block = LeafBlock {
        block_id: BlockId::zero(),
        height: NodeHeight(123),
        epoch,
    };
    tx.leaf_block_set(&leaf_block).unwrap();
    let res = tx.leaf_block_get(epoch).unwrap();
    assert_eq_debug(&res, &leaf_block);

    leaf_block.height += NodeHeight(1);

    tx.leaf_block_set(&leaf_block).unwrap();
    let res = tx.leaf_block_get(epoch).unwrap();
    assert_eq_debug(&res, &leaf_block);

    // high qc
    let epoch = Epoch::zero();
    let mut high_qc = HighQc {
        block_id: BlockId::zero(),
        epoch,
        block_height: NodeHeight(123),
        qc_id: QcId::zero(),
    };
    tx.high_qc_set(&high_qc).unwrap();
    let res = tx.high_qc_get(epoch).unwrap();
    assert_eq_debug(&res, &high_qc);

    high_qc.block_height += NodeHeight(1);

    tx.high_qc_set(&high_qc).unwrap();
    let res = tx.high_qc_get(epoch).unwrap();
    assert_eq_debug(&res, &high_qc);

    // epoch checkpoints
    let shard_group = ShardGroup::all_shards(NumPreshards::P4);
    let block = Block::zero_block(Default::default(), NumPreshards::P4);
    let qc = QuorumCertificate::genesis(Epoch::zero(), shard_group);
    let mut shard_roots = IndexMap::new();
    shard_roots.insert(shard_group.start(), TreeHash::zero());
    let epoch_checkpoint = EpochCheckpoint::new(block.clone(), vec![qc], shard_roots);

    tx.epoch_checkpoint_save(&epoch_checkpoint).unwrap();
    let res = tx.epoch_checkpoint_get(block.epoch()).unwrap();
    assert_eq_debug(&res, &epoch_checkpoint);

    // foreign parked blocks
    let justify_qc = QuorumCertificate::genesis(epoch, shard_group);
    let foreign_parked_block = ForeignParkedProposal::new(ForeignProposal {
        block: block.clone(),
        block_pledge: BlockPledge::new(),
        justify_qc,
        proposed_by_block: None,
        status: ForeignProposalStatus::New,
    });
    tx.foreign_parked_blocks_insert(&foreign_parked_block).unwrap();
    let res = tx
        .foreign_parked_blocks_exists(foreign_parked_block.block().id())
        .unwrap();
    assert!(res);

    tx.rollback().unwrap();
}
