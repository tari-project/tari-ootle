//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Read-only rollback queries that walk the rocksdb state store directly.
//!
//! These were previously methods on `StateStoreReadTransaction`. Consensus never calls
//! them, so they now live with the rollback tool and operate against the concrete
//! `RocksDbStateStoreReadTransaction` type.

use serde::{Serialize, de::DeserializeOwned};
use tari_consensus_types::BlockId;
use tari_ootle_common_types::{Epoch, NodeAddressable, NodeHeight, shard::Shard};
use tari_ootle_storage::{Ordering, StorageError, consensus_models::RollbackHistoryEntry};
use tari_state_store_rocksdb::{
    column_families::{
        block,
        block::BlockCf,
        rollback_history::RollbackHistoryCf,
        state_transition::{ByShardAndStateVersionQuery, StateTransitionCf, StateTransitionType},
        substate::SubstateCf,
    },
    reader::RocksDbStateStoreReadTransaction,
};
use tari_state_tree::Version;

use super::types::{BlocksAfterEpochRow, RewindTransitionKind, SubstateRewindPlanRow};

/// List all rollback-history breadcrumbs in chronological order (oldest first).
/// Returns an empty vec if no rollback has ever been applied.
pub fn rollback_history_list<TAddr>(
    tx: &RocksDbStateStoreReadTransaction<'_, TAddr>,
) -> Result<Vec<RollbackHistoryEntry>, StorageError>
where TAddr: NodeAddressable + Serialize + DeserializeOwned {
    const OPERATION: &str = "rollback_history_list";
    let cf = tx.db().cf(RollbackHistoryCf)?;
    let iter = cf.iterator(Ordering::Ascending, OPERATION);
    iter.map(|result| result.map(|(_, entry)| entry))
        .collect::<Result<_, _>>()
        .map_err(Into::into)
}

/// Mirrors the walk performed by `substates_rewind_to_state_version`: collect every
/// state_version > target for this shard in descending order, then for each record
/// yield one row per transition in *reverse index order* so the dry-run produces the
/// same reverse-application sequence the mutating rewind applies.
pub fn rollback_plan_collect_substates<'a, TAddr>(
    tx: &RocksDbStateStoreReadTransaction<'a, TAddr>,
    shard: Shard,
    target_version: Version,
) -> Result<Vec<SubstateRewindPlanRow>, StorageError>
where
    TAddr: NodeAddressable + Serialize + DeserializeOwned + 'a,
{
    const OPERATION: &str = "rollback_plan_collect_substates";

    let db = tx.db();
    let transitions_cf = db.cf(StateTransitionCf)?;
    let transitions_query = db.cf(ByShardAndStateVersionQuery)?;
    let substates_cf = db.cf(SubstateCf)?;

    let start_version = target_version.saturating_add(1);

    let versions: Vec<Version> = transitions_query
        .query_range_key_iterator(Ordering::Descending, (shard, start_version)..(shard, Version::MAX))
        .map(|res| {
            let (key_shard, version) = res?;
            debug_assert_eq!(key_shard, shard);
            Ok::<_, StorageError>(version)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut rows = Vec::new();
    for state_version in versions {
        let record = transitions_cf.get(&(shard, state_version), OPERATION)?;
        for transition in record.transitions.iter().rev() {
            let substate = substates_cf.get(&transition.substate_address, OPERATION)?;
            let kind = match transition.transition {
                StateTransitionType::Up => RewindTransitionKind::UpReverted,
                StateTransitionType::Down => RewindTransitionKind::DownReverted,
            };
            rows.push(SubstateRewindPlanRow {
                substate_id: substate.substate_id,
                shard,
                state_version,
                transition: kind,
                epoch: record.epoch,
            });
        }
    }
    Ok(rows)
}

/// Mirrors the block-collection step of `rollback_delete_after_epoch`: enumerate every
/// block with `epoch > target`, load each to extract the ids of transactions whose
/// finalising commit would be undone.
pub fn rollback_plan_collect_blocks<'a, TAddr>(
    tx: &RocksDbStateStoreReadTransaction<'a, TAddr>,
    target_epoch: Epoch,
) -> Result<Vec<BlocksAfterEpochRow>, StorageError>
where
    TAddr: NodeAddressable + Serialize + DeserializeOwned + 'a,
{
    const OPERATION: &str = "rollback_plan_collect_blocks";

    let db = tx.db();
    let block_cf = db.cf(BlockCf)?;
    let block_query = db.cf(block::ByEpochQuery)?;
    let start_epoch = target_epoch + Epoch(1);

    let keys: Vec<(Epoch, NodeHeight, BlockId)> = block_query
        .query_range_key_iterator(Ordering::Ascending, start_epoch..Epoch::max())
        .collect::<Result<Vec<_>, _>>()?;

    let mut rows = Vec::with_capacity(keys.len());
    for (epoch, _height, block_id) in keys {
        let block = block_cf.get(&block_id, OPERATION)?;
        let finalising_transaction_ids = block.all_finalising_transactions_ids().copied().collect();
        rows.push(BlocksAfterEpochRow {
            block_id,
            epoch,
            finalising_transaction_ids,
        });
    }
    Ok(rows)
}
