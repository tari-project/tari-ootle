//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Write-side rollback primitives that operate against the rocksdb state store directly.
//!
//! These were previously methods on `StateStoreWriteTransaction`. Consensus never calls
//! them, so they live with the rollback tool and take a `&mut RocksDbStateStoreWriteTransaction`.
//!
//! `rollback_delete_after_epoch` reaches into per-block trait helpers (block diffs,
//! block transaction executions, pending state-tree diffs, substate locks, lock
//! conflicts, transaction-pool state updates) — those helpers stay on
//! `StateStoreWriteTransaction` because consensus uses them as well, so we just call
//! through the trait via `&mut tx`.

use std::collections::HashSet;

use log::*;
use serde::{Serialize, de::DeserializeOwned};
use tari_consensus_types::{BlockId, PcId, TcId};
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{Epoch, NodeAddressable, NodeHeight, ShardGroup, optional::Optional, shard::Shard};
use tari_ootle_storage::{Ordering, StateStoreWriteTransaction, StorageError, consensus_models::RollbackHistoryEntry};
use tari_state_store_rocksdb::{
    codecs::ByteColumn,
    column_families::{
        block,
        block::BlockCf,
        block_transaction_execution::BlockTransactionExecutionCf,
        bookkeeping::{
            HighPcCf,
            HighTcCf,
            HighestSeenBlockCf,
            LastExecutedCf,
            LastProposedCf,
            LastSentNewViewCf,
            LastSentVoteCf,
            LastVotedCf,
            LeafBlockCf,
            LockedBlockCf,
        },
        certificates::{proposal::ProposalCertificateCf, timeout::TimeoutCertificateCf},
        chain::{self, CommittedParentChildChainIndex},
        epoch_checkpoint::EpochCheckpointCf,
        finalized_transaction::FinalizedTransactionLinkCf,
        foreign_proposal::{self, EpochIndex as ForeignProposalEpochIndex, ForeignProposalCf},
        rollback_history::RollbackHistoryCf,
        state_transition::{ByShardAndStateVersionQuery, StateTransitionCf, StateTransitionType},
        state_tree::{ByShardStateVersionQuery, ByStateTreeStaleShardVersionQuery, StateTreeCf, StateTreeStaleNodesCf},
        state_tree_shard_versions::StateTreeShardVersionCf,
        substate::{HeadIndex as SubstateHeadIndex, SubstateCf, SubstateHeadData, UnprunedDownedValuesIndex},
        validator_node_epoch_stats::ValidatorNodeEpochStatsCf,
    },
    writer::RocksDbStateStoreWriteTransaction,
};
use tari_state_tree::Version;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use super::types::{RollbackDeleteStats, StateTreeTruncateStats, SubstateRewindStats};

const LOG_TARGET: &str = "tari::ootle::validator_rollback::storage::write";

/// Append a rollback-history breadcrumb. Called inside the same write transaction as
/// the truncate / rewind / delete-after-epoch ops so either all four succeed or none do.
pub fn rollback_history_insert<'a, TAddr>(
    tx: &mut RocksDbStateStoreWriteTransaction<'a, TAddr>,
    entry: &RollbackHistoryEntry,
) -> Result<(), StorageError>
where
    TAddr: NodeAddressable + Serialize + DeserializeOwned + 'a,
{
    const OPERATION: &str = "rollback_history_insert";
    tx.db()
        .cf(RollbackHistoryCf)?
        .put(&(entry.applied_at_unix_secs, entry.target_epoch), entry, OPERATION)?;
    Ok(())
}

/// Truncate the state tree for `shard` back to `target_version`.
///
/// After this call:
/// - `state_tree_versions_get_latest(shard)` returns `target_version` (or `None` if the shard had no state at or below
///   `target_version`).
/// - All nodes at versions > `target_version` are deleted from the tree store.
/// - All stale-node records at versions > `target_version` are deleted.
pub fn state_tree_truncate_to_version<'a, TAddr>(
    tx: &mut RocksDbStateStoreWriteTransaction<'a, TAddr>,
    shard: Shard,
    target_version: Version,
) -> Result<StateTreeTruncateStats, StorageError>
where
    TAddr: NodeAddressable + Serialize + DeserializeOwned + 'a,
{
    const OPERATION: &str = "state_tree_truncate_to_version";

    let db = tx.db();
    let tree_cf = db.cf(StateTreeCf)?;
    let version_query = db.cf(ByShardStateVersionQuery)?;
    let stale_query = db.cf(ByStateTreeStaleShardVersionQuery)?;
    let stale_cf = db.cf(StateTreeStaleNodesCf)?;
    let versions_cf = db.cf(StateTreeShardVersionCf)?;

    // Versions are u64; saturate on the unlikely MAX case so we don't overflow.
    let start_version = target_version.saturating_add(1);

    // 1. Delete tree nodes at versions > target for this shard.
    let mut keys_to_delete = Vec::new();
    for result in version_query.query_range_iterator(Ordering::Ascending, (shard, start_version)..(shard, Version::MAX))
    {
        let ((key_shard, node_key), _node) = result?;
        debug_assert_eq!(key_shard, shard, "range iterator leaked across shard boundary");
        keys_to_delete.push((key_shard, node_key));
    }
    let nodes_deleted = keys_to_delete.len();
    for key in &keys_to_delete {
        tree_cf.delete(key, OPERATION)?;
    }

    // 2. Delete stale-node records at versions > target for this shard.
    let mut stale_keys_to_delete = Vec::new();
    for result in stale_query.query_range_iterator(Ordering::Ascending, (shard, start_version)..(shard, Version::MAX)) {
        let ((stale_shard, version), _nodes) = result?;
        debug_assert_eq!(stale_shard, shard);
        stale_keys_to_delete.push((stale_shard, version));
    }
    let stale_records_deleted = stale_keys_to_delete.len();
    for key in &stale_keys_to_delete {
        stale_cf.delete(key, OPERATION)?;
    }

    // 3. Reset the latest-version pointer. State versions start at 1, so an entry with value 0 is never valid: a shard
    //    with no committed state has *no entry*, and downstream readers (JMT root lookup, state-sync's
    //    `calculate_state_root_for_shard`) distinguish None (empty tree → placeholder hash) from Some(v) (load root at
    //    v). If we wrote 0 here, those readers would try to load a v0 root node that never existed and error with JMT
    //    NotFound.
    if target_version == 0 {
        versions_cf.delete(&shard, OPERATION)?;
    } else {
        versions_cf.put(&shard, &target_version, OPERATION)?;
    }

    debug!(
        target: LOG_TARGET,
        "Truncated state tree for shard {shard} to version {target_version}: \
         deleted {nodes_deleted} node(s), {stale_records_deleted} stale record(s)"
    );

    Ok(StateTreeTruncateStats {
        nodes_deleted,
        stale_records_deleted,
    })
}

/// Rewind substate state for `shard` back to `target_state_version` by inverting every
/// state transition recorded for versions > `target_state_version`.
///
/// Inverse operations:
/// - An `Up` transition → delete the `SubstateRecord` it created.
/// - A `Down` transition → clear the `destroyed` field on the affected `SubstateRecord`.
///
/// After the inverses are applied, the `HeadIndex` is rebuilt for every touched
/// `SubstateId` from the highest-version `SubstateRecord` that survives in storage.
pub fn substates_rewind_to_state_version<'a, TAddr>(
    tx: &mut RocksDbStateStoreWriteTransaction<'a, TAddr>,
    shard: Shard,
    target_state_version: Version,
) -> Result<SubstateRewindStats, StorageError>
where
    TAddr: NodeAddressable + Serialize + DeserializeOwned + 'a,
{
    const OPERATION: &str = "substates_rewind_to_state_version";
    use tari_state_store_rocksdb::codecs::KeyPrefix;

    let db = tx.db();
    let transitions_cf = db.cf(StateTransitionCf)?;
    let transitions_query = db.cf(ByShardAndStateVersionQuery)?;
    let substates_cf = db.cf(SubstateCf)?;
    let head_cf = db.cf(SubstateHeadIndex)?;
    let unpruned_cf = db.cf(UnprunedDownedValuesIndex)?;

    let start_version = target_state_version.saturating_add(1);
    let mut touched: HashSet<SubstateId> = HashSet::new();
    let mut stats = SubstateRewindStats::default();

    // Collect only the versions we need to process — then load each record one at a time
    // inside the loop. This caps memory at ~O(n_versions) rather than O(n_versions *
    // avg_record_size), which matters for large rollbacks spanning many epochs.
    let versions: Vec<Version> = transitions_query
        .query_range_key_iterator(Ordering::Descending, (shard, start_version)..(shard, Version::MAX))
        .map(|res| {
            let (key_shard, version) = res?;
            debug_assert_eq!(key_shard, shard);
            Ok::<_, StorageError>(version)
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Iterator was Descending, so `versions` is already in reverse state_version order.
    for state_version in versions {
        let record = transitions_cf.get(&(shard, state_version), OPERATION)?;
        // Within a record, invert transitions in reverse index order so that the inverse
        // sequence is the exact time-reversed mirror of forward application.
        for transition in record.transitions.iter().rev() {
            let address = &transition.substate_address;
            match transition.transition {
                StateTransitionType::Up => {
                    let substate = substates_cf.get(address, OPERATION)?;
                    touched.insert(substate.substate_id.clone());
                    substates_cf.delete(address, OPERATION)?;
                    stats.substates_created_deleted += 1;
                },
                StateTransitionType::Down => {
                    let mut substate = substates_cf.get(address, OPERATION)?;
                    touched.insert(substate.substate_id.clone());
                    substate.destroyed = None;
                    substates_cf.put(address, &substate, OPERATION)?;
                    stats.substates_destroyed_restored += 1;
                },
            }
        }

        // Remove the transition record and any unpruned-down index entry for this (epoch, shard, version).
        transitions_cf.delete(&(shard, state_version), OPERATION)?;
        unpruned_cf
            .delete(&(record.epoch, shard, state_version), OPERATION)
            .optional()?;
        stats.transitions_processed += 1;
    }

    // Rebuild the HeadIndex for every touched SubstateId by finding the highest surviving
    // version via a reverse prefix scan on SubstateCf. SubstateAddress = object_key ||
    // version_be, so all versions for a given substate_id are lexically adjacent.
    for substate_id in touched {
        let object_key = substate_id.to_object_key();
        let object_key_bytes: &[u8] = object_key.as_ref();
        let mut raw_prefix = Vec::with_capacity(1 + object_key_bytes.len());
        raw_prefix.push(KeyPrefix::Substates.as_u8());
        raw_prefix.extend_from_slice(object_key_bytes);

        let mut iter = substates_cf.prefix_range_iterator_raw_key(Ordering::Descending, raw_prefix);
        match iter.next() {
            Some(result) => {
                let (_address, record) = result?;
                head_cf.put(
                    &substate_id,
                    &SubstateHeadData {
                        version: record.version,
                        is_up: record.is_up(),
                    },
                    OPERATION,
                )?;
            },
            None => {
                head_cf.delete(&substate_id, OPERATION).optional()?;
            },
        }
        stats.heads_updated += 1;
    }

    debug!(
        target: LOG_TARGET,
        "🔄 Rewound substates for shard {shard} to state_version {target_state_version}: \
         {} transitions, {} created-deleted, {} destroyed-restored, {} heads updated",
        stats.transitions_processed,
        stats.substates_created_deleted,
        stats.substates_destroyed_restored,
        stats.heads_updated,
    );

    Ok(stats)
}

/// Delete every epoch-indexed record with `epoch > target_epoch`, and clear the singleton
/// bookkeeping pointers. Paired with `state_tree_truncate_to_version` and
/// `substates_rewind_to_state_version`, this is the storage side of a break-glass rollback
/// to the `target_epoch` checkpoint.
///
/// Caller invokes this *inside the same write transaction* as the state-tree truncate,
/// substate rewind, and `rollback_history_insert`.
///
/// After commit, consensus resumes via the `create_genesis_block_if_required` path in
/// hotstuff: with no blocks at `(current_epoch, height=0)` the worker creates a fresh
/// genesis and repopulates all bookkeeping pointers from it.
#[allow(clippy::too_many_lines)]
pub fn rollback_delete_after_epoch<'a, TAddr>(
    tx: &mut RocksDbStateStoreWriteTransaction<'a, TAddr>,
    target_epoch: Epoch,
) -> Result<RollbackDeleteStats, StorageError>
where
    TAddr: NodeAddressable + Serialize + DeserializeOwned + 'a,
{
    const OPERATION: &str = "rollback_delete_after_epoch";

    let mut stats = RollbackDeleteStats::default();
    let start_epoch = target_epoch + Epoch(1);

    // 1. Collect block_ids where epoch > target. We collect first so that per-block cascade helpers (which take `&mut
    //    self`) can run without holding an immutable CF-handle borrow over the iteration.
    let to_delete: Vec<(Epoch, NodeHeight, BlockId)> = tx
        .db()
        .cf(block::ByEpochQuery)?
        .query_range_key_iterator(Ordering::Ascending, start_epoch..Epoch::max())
        .collect::<Result<Vec<_>, _>>()?;

    for (epoch, height, block_id) in &to_delete {
        // Load the block before deleting so we can walk its commands and clean up per-tx
        // records that the block-id-keyed cascade below doesn't cover. Specifically,
        // `transactions_finalize_all` prunes the (block_id, tx_id, height) index on
        // finalize; that leaves `block_transaction_executions_remove_any_by_block_id`
        // with nothing to scan for already-finalized transactions, so their
        // `BlockTransactionExecutionCf` rows + `FinalizedTransactionLinkCf` link would
        // otherwise survive the rollback and let `get_transaction_result` keep returning
        // the committed execution.
        let block = tx.db().cf(BlockCf)?.get(block_id, OPERATION)?;

        // Per-block cascades via existing helpers. Keeps this list consistent with what
        // block insertion creates, so the cascade stays correct as new tables are added.
        // These helpers stay on `StateStoreWriteTransaction` because consensus also uses
        // them — we just call through the trait via `&mut tx`.
        tx.block_diffs_remove(block_id)?;
        tx.block_transaction_executions_remove_any_by_block_id(block_id)?;
        tx.pending_state_tree_diffs_remove_by_block(block_id)?;
        tx.substate_locks_remove_any_by_block_id(block_id)?;
        tx.transaction_pool_state_updates_remove_any_by_block_id(block_id)?;
        tx.lock_conflicts_remove_by_block_id(block_id)?;

        // Clean up finalized execution records for transactions in this block. The
        // `BlockTransactionExecutionCf` row is keyed by (tx_id, block_id, height) and is
        // inserted at the block where the transaction was executed (Prepare for
        // multi-stage, this block itself for LocalOnly). Attempting to delete at
        // (tx_id, this_block_id, this_block_height) is correct for LocalOnly rows and a
        // no-op otherwise — the Prepare block's own iteration will catch its own row.
        let exec_cf = tx.db().cf(BlockTransactionExecutionCf)?;
        let finalized_link_cf = tx.db().cf(FinalizedTransactionLinkCf)?;
        for tx_id in block.all_transaction_ids() {
            exec_cf.delete(&(*tx_id, *block_id, *height), OPERATION)?;
        }
        // Finalising commands mark this block as where the transaction's finalize QC
        // committed, so `FinalizedTransactionLinkCf[tx_id]` was written here. Deleting it
        // restores the "not-yet-finalized" state so `finalized_transaction_execution_get`
        // returns NotFound post-rollback.
        for tx_id in block.all_finalising_transactions_ids() {
            finalized_link_cf.delete(tx_id, OPERATION)?;
        }

        // Delete the block record and its chain/epoch indexes. Use the idempotent
        // `delete` throughout — a block is either in both chain indexes or neither (it
        // was committed vs. still pending), so one of `CommittedParentChildChainIndex`
        // / `PendingChainIndex` will always be a miss and the `delete_or_not_found`
        // variant would panic on that miss.
        tx.db().cf(BlockCf)?.delete(block_id, OPERATION)?;
        tx.db()
            .cf(block::EpochHeightIndex)?
            .delete(&(*epoch, *height, *block_id), OPERATION)?;
        tx.db()
            .cf(CommittedParentChildChainIndex)?
            .delete(block_id, OPERATION)?;
        tx.db().cf(chain::PendingChainIndex)?.delete(block_id, OPERATION)?;
        // PendingParentChildIndex is keyed by (parent, child). Precise deletion needs the
        // block's parent, which itself may already be gone. Stale entries are benign —
        // all index consumers start from known block_ids and won't reach orphans.
    }
    stats.blocks_deleted = to_delete.len();

    // 2. Proposal certificates with epoch > target.
    let pc_cf = tx.db().cf(ProposalCertificateCf)?;
    let pc_query = tx
        .db()
        .cf(tari_state_store_rocksdb::column_families::certificates::proposal::ByEpochQuery)?;
    let pc_keys: Vec<(Epoch, PcId)> = pc_query
        .query_range_key_iterator(Ordering::Ascending, start_epoch..Epoch::max())
        .collect::<Result<Vec<_>, _>>()?;
    for key in &pc_keys {
        pc_cf.delete(key, OPERATION)?;
    }
    stats.certificates_deleted += pc_keys.len();

    // 3. Timeout certificates with epoch > target.
    let tc_cf = tx.db().cf(TimeoutCertificateCf)?;
    let tc_query = tx
        .db()
        .cf(tari_state_store_rocksdb::column_families::certificates::timeout::ByEpochQuery)?;
    let tc_keys: Vec<(Epoch, TcId)> = tc_query
        .query_range_key_iterator(Ordering::Ascending, start_epoch..Epoch::max())
        .collect::<Result<Vec<_>, _>>()?;
    for key in &tc_keys {
        tc_cf.delete(key, OPERATION)?;
    }
    stats.certificates_deleted += tc_keys.len();

    // 4. Epoch checkpoints with epoch > target.
    let cp_cf = tx.db().cf(EpochCheckpointCf)?;
    let cp_query = tx
        .db()
        .cf(tari_state_store_rocksdb::column_families::epoch_checkpoint::ByEpochQuery)?;
    let cp_keys: Vec<(Epoch, ShardGroup)> = cp_query
        .query_range_key_iterator(Ordering::Ascending, start_epoch..Epoch::max())
        .collect::<Result<Vec<_>, _>>()?;
    for key in &cp_keys {
        cp_cf.delete(key, OPERATION)?;
    }
    stats.checkpoints_deleted = cp_keys.len();

    // 5. Foreign proposals with epoch > target. Use the ByEpochQuery to find block_ids, then delete via the existing
    //    per-block helper which handles all the secondary indexes.
    let fp_query = tx.db().cf(foreign_proposal::ByEpochQuery)?;
    let fp_epoch_index_cf = tx.db().cf(ForeignProposalEpochIndex)?;
    let fp_epoch_keys: Vec<((Epoch, BlockId), foreign_proposal::ForeignProposalEpochIndexData)> = fp_query
        .query_range_iterator(Ordering::Ascending, start_epoch..Epoch::max())
        .collect::<Result<Vec<_>, _>>()?;
    for ((epoch, block_id), _) in &fp_epoch_keys {
        // Idempotent delete in case a prior cascade already removed it.
        tx.db().cf(ForeignProposalCf)?.delete(block_id, OPERATION)?;
        fp_epoch_index_cf.delete(&(*epoch, *block_id), OPERATION)?;
    }
    stats.foreign_proposals_deleted = fp_epoch_keys.len();

    // 6. Validator epoch stats with epoch > target.
    let stats_cf = tx.db().cf(ValidatorNodeEpochStatsCf)?;
    let stats_query = tx
        .db()
        .cf(tari_state_store_rocksdb::column_families::validator_node_epoch_stats::ByEpochQuery)?;
    let stats_keys: Vec<(Epoch, RistrettoPublicKeyBytes)> = stats_query
        .query_range_key_iterator(Ordering::Ascending, start_epoch..Epoch::max())
        .collect::<Result<Vec<_>, _>>()?;
    for key in &stats_keys {
        stats_cf.delete(key, OPERATION)?;
    }
    stats.validator_stats_deleted = stats_keys.len();

    // 7. Clear bookkeeping singletons. These are all keyed by `ByteColumn`; deleting the single entry removes the
    //    pointer entirely. On consensus resume the genesis path in `create_genesis_block_if_required` repopulates them
    //    for the fresh epoch. Use idempotent `delete` — a freshly-joined node may not have every singleton populated
    //    yet (e.g. `LastSentVote` before it has ever voted) and this step must not fail in that case.
    tx.db().cf(LeafBlockCf)?.delete(&ByteColumn, OPERATION)?;
    tx.db().cf(LockedBlockCf)?.delete(&ByteColumn, OPERATION)?;
    tx.db().cf(HighestSeenBlockCf)?.delete(&ByteColumn, OPERATION)?;
    tx.db().cf(LastExecutedCf)?.delete(&ByteColumn, OPERATION)?;
    tx.db().cf(LastVotedCf)?.delete(&ByteColumn, OPERATION)?;
    tx.db().cf(LastProposedCf)?.delete(&ByteColumn, OPERATION)?;
    tx.db().cf(LastSentVoteCf)?.delete(&ByteColumn, OPERATION)?;
    tx.db().cf(LastSentNewViewCf)?.delete(&ByteColumn, OPERATION)?;
    tx.db().cf(HighPcCf)?.delete(&ByteColumn, OPERATION)?;
    tx.db().cf(HighTcCf)?.delete(&ByteColumn, OPERATION)?;
    stats.bookkeeping_cleared = true;

    info!(
        target: LOG_TARGET,
        "🔄 Rollback delete for epoch > {target_epoch}: {} blocks, {} certificates, \
         {} checkpoints, {} foreign proposals, {} validator-stats rows; bookkeeping cleared",
        stats.blocks_deleted,
        stats.certificates_deleted,
        stats.checkpoints_deleted,
        stats.foreign_proposals_deleted,
        stats.validator_stats_deleted,
    );

    Ok(stats)
}
