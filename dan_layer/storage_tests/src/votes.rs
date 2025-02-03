//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_storage::{
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};

mod last_inserted {
    use tari_dan_common_types::Epoch;
    use tari_dan_storage::consensus_models::{QuorumDecision, Vote};

    use crate::helper::{assert_eq_debug, create_random_block_id, create_random_hash, create_random_vn_signature, create_rocksdb, create_sqlite};
    
    use super::*;

    #[test]
    fn votes_sqlite() {
        let db = create_sqlite();
        db.foreign_keys_off().unwrap();
        votes_operations(db);
    }

    #[test]
    fn votes_rocksdb() {
        let db = create_rocksdb();
        votes_operations(db);
    }

    fn votes_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();

        // votes_insert
        let vote_1 = create_random_vote();
        tx.votes_insert(&vote_1).unwrap();
        let vote_2 = create_random_vote();
        tx.votes_insert(&vote_2).unwrap();
        let vote_3 = create_random_vote();
        tx.votes_insert(&vote_3).unwrap();


        // votes_get_by_block_and_sender
        let res = tx.votes_get_by_block_and_sender(&vote_1.block_id, &vote_1.sender_leaf_hash).unwrap();
        assert_eq_debug(&res, &vote_1);
        let res = tx.votes_get_by_block_and_sender(&vote_2.block_id, &vote_2.sender_leaf_hash).unwrap();
        assert_eq_debug(&res, &vote_2);
        let res = tx.votes_get_by_block_and_sender(&vote_3.block_id, &vote_3.sender_leaf_hash).unwrap();
        assert_eq_debug(&res, &vote_3);

        // votes_count_for_block
        let res = tx.votes_count_for_block(&vote_1.block_id).unwrap();
        assert_eq!(res, 1);

        // votes_get_for_block
        let res = tx.votes_get_for_block(&vote_1.block_id).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq_debug(&res[0], &vote_1);

        // votes_delete_all
        tx.votes_delete_all().unwrap();
        let res = tx.votes_get_for_block(&vote_1.block_id).unwrap();
        assert_eq!(res.len(), 0);

        tx.rollback().unwrap();
    }

    fn create_random_vote() -> Vote {
        Vote {
            epoch: Epoch::zero(),
            block_id: create_random_block_id(),
            decision: QuorumDecision::Accept,
            sender_leaf_hash: create_random_hash(),
            signature: create_random_vn_signature(),
        }
    }
}
