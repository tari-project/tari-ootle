//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Version 2 stores each uncommitted block's substate changes as one record per block, and its pending state tree
//! diffs as another, in place of version 1's record per change (with a by-substate index) and record per shard. This
//! migration moves every version 1 entry into its block's records, in order, and deletes the version 1 tables.

use std::collections::HashMap;

use log::*;
use tari_consensus_types::BlockId;
use tari_ootle_common_types::{NodeAddressable, shard::Shard};
use tari_ootle_storage::{
    Ordering,
    StateStoreWriteTransaction,
    consensus_models::{PendingShardStateTreeDiff, SubstateChange},
};
use tari_state_store_rocksdb::{
    cf_api::CfContext,
    column_families::{
        block_diff::legacy::{BlockDiffCf, SubstateIdIndex},
        pending_state_tree_diff::legacy::PendingStateTreeDiffCf,
    },
    error::RocksDbStorageError,
    traits::{Cf, RocksReader, RocksWriter},
    writer::RocksDbStateStoreWriteTransaction,
};

const LOG_TARGET: &str = "tari::validator_node::migrations::v2";

pub fn migrate<TAddr: NodeAddressable + 'static>(
    tx: &mut RocksDbStateStoreWriteTransaction<'_, TAddr>,
) -> anyhow::Result<()> {
    const OPERATION: &str = "migrate_v2";

    let mut changes_by_block = HashMap::<BlockId, Vec<(u32, SubstateChange)>>::new();
    let mut tree_diffs_by_block = HashMap::<BlockId, Vec<(Shard, PendingShardStateTreeDiff)>>::new();
    {
        let db = tx.db();
        for result in db.cf(BlockDiffCf)?.iterator(Ordering::Ascending, OPERATION) {
            let (key, change) = result?;
            changes_by_block
                .entry(key.block_id)
                .or_default()
                .push((key.sequence, change));
        }
        for result in db.cf(PendingStateTreeDiffCf)?.iterator(Ordering::Ascending, OPERATION) {
            let ((block_id, shard), diff) = result?;
            tree_diffs_by_block.entry(block_id).or_default().push((shard, diff));
        }
        delete_all(&db.cf(BlockDiffCf)?)?;
        delete_all(&db.cf(SubstateIdIndex)?)?;
        delete_all(&db.cf(PendingStateTreeDiffCf)?)?;
    }

    let num_diff_blocks = changes_by_block.len();
    for (block_id, mut changes) in changes_by_block {
        changes.sort_by_key(|(sequence, _)| *sequence);
        let changes = changes.into_iter().map(|(_, change)| change).collect::<Vec<_>>();
        tx.block_diffs_insert(&block_id, &changes)?;
    }
    let num_tree_diff_blocks = tree_diffs_by_block.len();
    for (block_id, diffs) in tree_diffs_by_block {
        tx.pending_state_tree_diffs_insert_all(&block_id, diffs.iter().map(|(shard, diff)| (shard, diff)))?;
    }

    info!(
        target: LOG_TARGET,
        "Moved the substate changes of {num_diff_blocks} block(s) and the pending state tree diffs of \
         {num_tree_diff_blocks} block(s) into per-block records"
    );
    Ok(())
}

fn delete_all<CF: Cf, DB: RocksReader + RocksWriter>(cf: &CfContext<'_, DB, CF>) -> Result<(), RocksDbStorageError> {
    const OPERATION: &str = "migrate_v2::delete_all";
    let keys = cf
        .key_iterator(Ordering::Ascending, OPERATION)
        .collect::<Result<Vec<_>, _>>()?;
    for key in keys {
        cf.delete(&key, OPERATION)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tari_engine_types::substate::SubstateId;
    use tari_ootle_common_types::{SubstateVersion, VersionedSubstateId};
    use tari_ootle_storage::{StateStore, StateStoreReadTransaction, StorageError};
    use tari_ootle_transaction::TransactionId;
    use tari_state_store_rocksdb::{
        DatabaseOptions,
        RocksDbStateStore,
        column_families::{
            block_diff::{BlockDiffRecordCf, legacy::BlockDiffKey},
            pending_state_tree_diff::PendingStateTreeDiffRecordCf,
        },
    };
    use tari_state_tree::StateHashTreeDiff;

    use super::*;

    /// Every version 1 change lands in its block's record in sequence order, every version 1 tree diff in its block's
    /// record, and none of the version 1 tables survive.
    #[test]
    fn version_1_block_diffs_are_moved_into_block_records() {
        const OPERATION: &str = "test";
        let tmp = tempfile::tempdir().unwrap();
        let db = RocksDbStateStore::<String>::open(tmp.path().join("rocksdb"), DatabaseOptions::default()).unwrap();
        let block_id = BlockId::new([7u8; 32]);
        let substate_id = SubstateId::TransactionReceipt(TransactionId::new([9; 32]).into());
        let shard = Shard::from(3u32);
        // Written out of sequence order, so the record's order can only come from the sequence numbers.
        let changes = [(1, 1u64), (0, 0), (2, 2)];

        db.with_write_tx(|tx| {
            let db = tx.db();
            for (sequence, version) in changes {
                let key = BlockDiffKey {
                    block_id,
                    substate_id: substate_id.clone(),
                    version: SubstateVersion::new(version),
                    is_up: false,
                    sequence,
                };
                let change = SubstateChange::Down {
                    id: VersionedSubstateId::new(substate_id.clone(), SubstateVersion::new(version)),
                    shard,
                };
                db.cf(BlockDiffCf)?.put(&key, &change, OPERATION)?;
                db.cf(SubstateIdIndex)?.put(&key, &(), OPERATION)?;
            }
            db.cf(PendingStateTreeDiffCf)?.put(
                &(block_id, shard),
                &PendingShardStateTreeDiff::new(5, StateHashTreeDiff::new()),
                OPERATION,
            )?;
            Ok::<_, StorageError>(())
        })
        .unwrap();

        db.with_write_tx(migrate).unwrap();

        let tx = db.create_read_tx().unwrap();
        let diff = tx.block_diffs_get(&block_id).unwrap();
        let order = diff
            .changes
            .iter()
            .map(|c| c.versioned_substate_id().version().as_u64())
            .collect::<Vec<_>>();
        assert_eq!(order, vec![0, 1, 2]);

        let db_ctx = tx.db();
        assert!(
            db_ctx
                .cf(BlockDiffRecordCf)
                .unwrap()
                .exists(&block_id, OPERATION)
                .unwrap()
        );
        let tree_diffs = db_ctx
            .cf(PendingStateTreeDiffRecordCf)
            .unwrap()
            .get(&block_id, OPERATION)
            .unwrap();
        assert_eq!(tree_diffs.len(), 1);
        assert_eq!(tree_diffs[0].shard, shard);
        assert_eq!(tree_diffs[0].diff.version, 5);

        assert_eq!(db_ctx.cf(BlockDiffCf).unwrap().count(OPERATION).unwrap(), 0);
        assert_eq!(db_ctx.cf(SubstateIdIndex).unwrap().count(OPERATION).unwrap(), 0);
        assert_eq!(db_ctx.cf(PendingStateTreeDiffCf).unwrap().count(OPERATION).unwrap(), 0);
    }
}
