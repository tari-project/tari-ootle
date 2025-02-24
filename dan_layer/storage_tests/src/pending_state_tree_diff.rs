//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_storage::{
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};

mod pending_state_tree_diffs {
    use tari_dan_storage::consensus_models::VersionedStateHashTreeDiff;
    use tari_state_tree::StateHashTreeDiff;

    use crate::helper::{create_block, create_rocksdb, create_sqlite};
    
    use super::*;

    #[test]
    fn pending_state_tree_diff_sqlite() {
        let db = create_sqlite();
        db.foreign_keys_off().unwrap();
        pending_state_tree_diff_operations(db);
    }

    #[test]
    fn pending_state_tree_diff_rocksdb() {
        let db = create_rocksdb();
        pending_state_tree_diff_operations(db);
    }

    fn pending_state_tree_diff_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();

        // add some (committed) blocks to the database
        let mut genesis = create_block(None);
        genesis.set_is_committed(true);
        genesis.insert(&mut tx).unwrap();

        let mut block_1 = create_block(Some(&genesis));
        block_1.set_is_committed(true);
        block_1.insert(&mut tx).unwrap();

        let block_2 = create_block(Some(&block_1));
        block_2.insert(&mut tx).unwrap();

        let block_3 = create_block(Some(&block_2));
        block_3.insert(&mut tx).unwrap();

        // pending_state_tree_diffs_insert
        let shard = block_2.shard_group().shard_iter().next().unwrap();
        let diff = VersionedStateHashTreeDiff::new(0, StateHashTreeDiff::new());
        tx.pending_state_tree_diffs_insert(*block_2.id(), shard, &diff).unwrap();

        // pending_state_tree_diffs_get_all_up_to_commit_block
        let res = tx.pending_state_tree_diffs_get_all_up_to_commit_block(block_3.id()).unwrap();
        assert_eq!(res.len(), 1);

        // pending_state_tree_diffs_remove_and_return_by_block
        let res = tx.pending_state_tree_diffs_remove_and_return_by_block(block_2.id()).unwrap();
        assert_eq!(res.len(), 1);
        let res = tx.pending_state_tree_diffs_get_all_up_to_commit_block(block_3.id()).unwrap();
        assert_eq!(res.len(), 0);

        tx.rollback().unwrap();
    }
}
