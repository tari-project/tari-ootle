//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::{Epoch, NodeHeight};
use tari_dan_storage::{
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};

mod miscellaneous_operations {
    use indexmap::IndexMap;
    use tari_common_types::types::PublicKey;
    use tari_dan_common_types::{shard::Shard, NumPreshards, ShardGroup};
    use tari_dan_storage::consensus_models::{Block, BlockId, BlockPledge, EpochCheckpoint, ForeignParkedProposal, ForeignProposal, ForeignProposalStatus, ForeignReceiveCounters, ForeignSendCounters, HighQc, LastExecuted, LastSentVote, LastVoted, LeafBlock, LockedBlock, QcId, QuorumCertificate, QuorumDecision, ValidatorSchnorrSignature, ValidatorSignature};
    use tari_state_tree::{Node, NodeKey, StaleTreeNode, TreeHash};

    use crate::helper::{assert_eq_debug, create_rocksdb, create_sqlite};
    
    use super::*;

    #[test]
    fn miscellaneous_sqlite() {
        let db = create_sqlite();
        db.foreign_keys_off().unwrap();
        miscellaneous_operations(db);
    }

    #[test]
    fn miscellaneous_rocksdb() {
        let db = create_rocksdb();
        miscellaneous_operations(db);
    }

    fn miscellaneous_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();

        // last voted
        let mut last_voted = LastVoted {
            block_id: BlockId::genesis(),
            height: NodeHeight(123),
            epoch: Epoch::zero(),
        };
        tx.last_voted_set(&last_voted).unwrap();
        let res = tx.last_voted_get().unwrap();
        assert_eq_debug(&res, &last_voted);

        last_voted.epoch = last_voted.epoch + Epoch(1);

        tx.last_voted_set(&last_voted).unwrap();
        let res = tx.last_voted_get().unwrap();
        assert_eq_debug(&res, &last_voted);

        // last sent vote
        let mut last_sent_vote = LastSentVote {
            block_id: BlockId::genesis(),
            epoch: Epoch::zero(),
            block_height: NodeHeight(123),
            decision: QuorumDecision::Accept,
            signature: ValidatorSignature::new(PublicKey::default(), ValidatorSchnorrSignature::default()),
        };
        tx.last_sent_vote_set(&last_sent_vote).unwrap();
        let res = tx.last_sent_vote_get().unwrap();
        assert_eq_debug(&res, &last_sent_vote);

        last_sent_vote.epoch = last_sent_vote.epoch + Epoch(1);

        tx.last_sent_vote_set(&last_sent_vote).unwrap();
        let res = tx.last_sent_vote_get().unwrap();
        assert_eq_debug(&res, &last_sent_vote);

        // last executed
        let mut last_exec = LastExecuted {
            block_id: BlockId::genesis(),
            height: NodeHeight(123),
            epoch: Epoch::zero(),
        };
        tx.last_executed_set(&last_exec).unwrap();
        let res = tx.last_executed_get().unwrap();
        assert_eq_debug(&res, &last_exec);

        last_exec.epoch = last_exec.epoch + Epoch(1);

        tx.last_executed_set(&last_exec).unwrap();
        let res = tx.last_executed_get().unwrap();
        assert_eq_debug(&res, &last_exec);

        // last executed
        let mut last_exec = LastExecuted {
            block_id: BlockId::genesis(),
            height: NodeHeight(123),
            epoch: Epoch::zero(),
        };
        tx.last_executed_set(&last_exec).unwrap();
        let res = tx.last_executed_get().unwrap();
        assert_eq_debug(&res, &last_exec);

        last_exec.epoch = last_exec.epoch + Epoch(1);

        tx.last_executed_set(&last_exec).unwrap();
        let res = tx.last_executed_get().unwrap();
        assert_eq_debug(&res, &last_exec);

        // locked block
        let epoch = Epoch::zero();
        let mut locked_block = LockedBlock {
            block_id: BlockId::genesis(),
            height: NodeHeight(123),
            epoch,
        };
        tx.locked_block_set(&locked_block).unwrap();
        let res = tx.locked_block_get(epoch).unwrap();
        assert_eq_debug(&res, &locked_block);

        locked_block.height = locked_block.height + NodeHeight(1);

        tx.locked_block_set(&locked_block).unwrap();
        let res = tx.locked_block_get(epoch).unwrap();
        assert_eq_debug(&res, &locked_block);

        // leaf block
        let epoch = Epoch::zero();
        let mut leaf_block = LeafBlock {
            block_id: BlockId::genesis(),
            height: NodeHeight(123),
            epoch,
        };
        tx.leaf_block_set(&leaf_block).unwrap();
        let res = tx.leaf_block_get(epoch).unwrap();
        assert_eq_debug(&res, &leaf_block);

        leaf_block.height = leaf_block.height + NodeHeight(1);

        tx.leaf_block_set(&leaf_block).unwrap();
        let res = tx.leaf_block_get(epoch).unwrap();
        assert_eq_debug(&res, &leaf_block);

        // high qc
        let epoch = Epoch::zero();
        let mut high_qc = HighQc {
            block_id: BlockId::genesis(),
            epoch,
            block_height: NodeHeight(123),
            qc_id: QcId::zero(),
        };
        tx.high_qc_set(&high_qc).unwrap();
        let res = tx.high_qc_get(epoch).unwrap();
        assert_eq_debug(&res, &high_qc);

        high_qc.block_height = high_qc.block_height + NodeHeight(1);

        tx.high_qc_set(&high_qc).unwrap();
        let res = tx.high_qc_get(epoch).unwrap();
        assert_eq_debug(&res, &high_qc);

        // foreign send counters
        let shard = Shard::zero();
        let block_id = BlockId::genesis();
        let mut counter = ForeignSendCounters::new();
        counter.increment_counter(shard);

        tx.foreign_send_counters_set(&counter, &block_id).unwrap();
        let res = tx.foreign_send_counters_get(&block_id).unwrap();
        assert_eq_debug(&res, &counter);

        // foreign receive counters
        let shard_group = ShardGroup::all_shards(NumPreshards::P1);
        let mut counter = ForeignReceiveCounters::new();
        counter.increment_group(shard_group);

        tx.foreign_receive_counters_set(&counter).unwrap();
        let res = tx.foreign_receive_counters_get().unwrap();
        assert_eq_debug(&res, &counter);

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
        let foreign_parked_block = ForeignParkedProposal::new(
            ForeignProposal {
                block: block.clone(),
                block_pledge: BlockPledge::new(),
                justify_qc,
                proposed_by_block: None,
                status: ForeignProposalStatus::New,
            });
        tx.foreign_parked_blocks_insert(&foreign_parked_block).unwrap();
        let res = tx.foreign_parked_blocks_exists(foreign_parked_block.block().id()).unwrap();
        assert!(res);

        // state_tree
        let node = Node::Null;
        let node_key = NodeKey::new_empty_path(0);
        tx.state_tree_nodes_insert(shard, node_key.clone(), node.clone()).unwrap();
        let res = tx.state_tree_nodes_get(shard, &node_key).unwrap();
        assert_eq_debug(&res, &node);

        let stale_node = StaleTreeNode::Node(node_key.clone());
        tx.state_tree_nodes_record_stale_tree_node(shard, stale_node).unwrap();
        let res = tx.state_tree_nodes_get(shard, &node_key);
        assert!(res.is_err());   

        tx.rollback().unwrap();
    }
}
