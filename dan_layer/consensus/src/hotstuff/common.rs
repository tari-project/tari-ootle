//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, ops::ControlFlow};

use indexmap::IndexMap;
use log::*;
use tari_common::configuration::Network;
use tari_common_types::types::FixedHash;
use tari_dan_common_types::{
    committee::{Committee, CommitteeInfo},
    derive_fee_pool_address,
    shard::Shard,
    substate_type::SubstateType,
    Epoch,
    NodeAddressable,
    NodeHeight,
    NumPreshards,
    ShardGroup,
};
use tari_dan_storage::{
    consensus_models::{
        Block,
        BlockHeader,
        BlockId,
        EpochCheckpoint,
        LeafBlock,
        PendingShardStateTreeDiff,
        QuorumCertificate,
        SubstateChange,
        ValidatorConsensusStats,
    },
    StateStore,
    StateStoreReadTransaction,
    StorageError,
};
use tari_engine_types::{substate::SubstateDiff, template_models::Amount, ValidatorFeePool};
use tari_state_tree::{JellyfishMerkleTree, StateTreeError, SPARSE_MERKLE_PLACEHOLDER_HASH};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    hotstuff::{
        substate_store::{PendingSubstateStore, ShardScopedTreeStoreReader, ShardedStateTree},
        HotStuffError,
    },
    traits::LeaderStrategy,
};

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::common";

/// Calculates the dummy block required to reach the new height and returns the last dummy block (parent for next
/// proposal). Generates dummy blocks from from_height to new_height _exclusive_.
pub fn calculate_last_dummy_block<TAddr: NodeAddressable, TLeaderStrategy: LeaderStrategy<TAddr>>(
    from_height: NodeHeight,
    new_height: NodeHeight,
    network: Network,
    epoch: Epoch,
    shard_group: ShardGroup,
    parent_block_id: BlockId,
    qc: &QuorumCertificate,
    parent_merkle_root: FixedHash,
    leader_strategy: &TLeaderStrategy,
    local_committee: &Committee<TAddr>,
    parent_timestamp: u64,
    parent_epoch_hash: FixedHash,
) -> Option<LeafBlock> {
    let mut dummy = None;
    with_dummy_blocks(
        from_height,
        new_height,
        network,
        epoch,
        shard_group,
        parent_block_id,
        qc,
        parent_merkle_root,
        leader_strategy,
        local_committee,
        parent_timestamp,
        parent_epoch_hash,
        |dummy_block| {
            dummy = Some(dummy_block.as_leaf_block());
            ControlFlow::Continue(())
        },
    );

    dummy
}

pub fn calculate_dummy_blocks<TAddr: NodeAddressable, TLeaderStrategy: LeaderStrategy<TAddr>>(
    from_height: NodeHeight,
    new_height: NodeHeight,
    network: Network,
    epoch: Epoch,
    shard_group: ShardGroup,
    parent_block_id: BlockId,
    qc: &QuorumCertificate,
    expected_parent_block_id: &BlockId,
    parent_merkle_root: FixedHash,
    leader_strategy: &TLeaderStrategy,
    local_committee: &Committee<TAddr>,
    parent_timestamp: u64,
    parent_epoch_hash: FixedHash,
) -> Vec<Block> {
    let mut dummies = Vec::with_capacity(new_height.saturating_sub(from_height).as_u64() as usize);
    with_dummy_blocks(
        from_height,
        new_height,
        network,
        epoch,
        shard_group,
        parent_block_id,
        qc,
        parent_merkle_root,
        leader_strategy,
        local_committee,
        parent_timestamp,
        parent_epoch_hash,
        |dummy_block| {
            if dummy_block.id() == expected_parent_block_id {
                dummies.push(dummy_block);
                ControlFlow::Break(())
            } else {
                dummies.push(dummy_block);
                ControlFlow::Continue(())
            }
        },
    );

    dummies
}

/// Calculates the dummy block required to reach the new height
pub fn calculate_dummy_blocks_from_justify<TAddr: NodeAddressable, TLeaderStrategy: LeaderStrategy<TAddr>>(
    candidate_block: &Block,
    justify_block: &Block,
    leader_strategy: &TLeaderStrategy,
    local_committee: &Committee<TAddr>,
) -> Vec<Block> {
    calculate_dummy_blocks(
        justify_block.height(),
        candidate_block.height(),
        candidate_block.network(),
        candidate_block.epoch(),
        candidate_block.shard_group(),
        *justify_block.id(),
        candidate_block.justify(),
        candidate_block.parent(),
        *justify_block.state_merkle_root(),
        leader_strategy,
        local_committee,
        justify_block.timestamp(),
        *justify_block.epoch_hash(),
    )
}

fn with_dummy_blocks<TAddr, TLeaderStrategy, F>(
    mut current_height: NodeHeight,
    new_height: NodeHeight,
    network: Network,
    epoch: Epoch,
    shard_group: ShardGroup,
    mut parent_block_id: BlockId,
    qc: &QuorumCertificate,
    parent_merkle_root: FixedHash,
    leader_strategy: &TLeaderStrategy,
    local_committee: &Committee<TAddr>,
    parent_timestamp: u64,
    parent_epoch_hash: FixedHash,
    mut callback: F,
) where
    TAddr: NodeAddressable,
    TLeaderStrategy: LeaderStrategy<TAddr>,
    F: FnMut(Block) -> ControlFlow<()>,
{
    if current_height >= new_height {
        error!(
            target: LOG_TARGET,
            "BUG: 🍼 no dummy blocks to calculate. current height {} is greater than new height {}",
            current_height,
            new_height,
        );
        return;
    }

    debug!(
        target: LOG_TARGET,
        "🍼 calculating dummy blocks in epoch {} from {} to {}",
        epoch,
        current_height,
        new_height,
    );
    loop {
        current_height += NodeHeight(1);
        if current_height == new_height {
            break;
        }
        let (_, leader) = leader_strategy.get_leader(local_committee, current_height);
        let dummy_header = BlockHeader::dummy_block(
            network,
            parent_block_id,
            *leader,
            current_height,
            *qc.id(),
            epoch,
            shard_group,
            parent_merkle_root,
            parent_timestamp,
            parent_epoch_hash,
        );
        let dummy_block = Block::new(dummy_header, qc.clone(), Default::default());
        debug!(
            target: LOG_TARGET,
            "🍼 new dummy block: {}",
            dummy_block,
        );
        parent_block_id = *dummy_block.id();

        if callback(dummy_block).is_break() {
            break;
        }
    }
}

pub fn calculate_state_merkle_root<'a, TTx: StateStoreReadTransaction, I: IntoIterator<Item = &'a SubstateChange>>(
    tx: &TTx,
    shard_group: ShardGroup,
    pending_tree_diffs: HashMap<Shard, Vec<PendingShardStateTreeDiff>>,
    changes: I,
) -> Result<(FixedHash, IndexMap<Shard, PendingShardStateTreeDiff>), StateTreeError> {
    let mut change_map = IndexMap::new();

    changes.into_iter().for_each(|ch| {
        // Group by shard
        change_map.entry(ch.shard()).or_insert_with(Vec::new).push(ch.into());
    });
    let mut sharded_tree = ShardedStateTree::new(tx).with_pending_diffs(pending_tree_diffs);
    let root_hash = sharded_tree.put_substate_tree_changes(shard_group, change_map)?;

    Ok((
        FixedHash::new(root_hash.into_array()),
        sharded_tree.into_shard_tree_diffs(),
    ))
}

pub(crate) fn generate_epoch_checkpoint<TTx>(
    tx: &TTx,
    epoch: Epoch,
    shard_group: ShardGroup,
) -> Result<EpochCheckpoint, HotStuffError>
where
    TTx: StateStoreReadTransaction,
{
    // Get the last 3 blocks in the previous epoch. These blocks should end the epoch.
    let mut blocks = Block::get_last_n_in_epoch(tx, 3, epoch)?;
    if blocks.is_empty() {
        return Err(HotStuffError::StorageError(StorageError::NotFound {
            item: "Block::get_last_n_in_epoch",
            key: epoch.to_string(),
        }));
    }

    let commit_block = blocks.pop().unwrap();
    let qcs = blocks.into_iter().map(|b| b.into_justify()).collect();

    // Fetch the state roots of the shards in the shard group
    let mut shard_roots = IndexMap::with_capacity(shard_group.len() + 1);

    // adding global shard first
    if let Some(version) = tx.state_tree_versions_get_latest(Shard::global())? {
        let scoped_store = ShardScopedTreeStoreReader::new(tx, Shard::global());
        let jmt = JellyfishMerkleTree::new(&scoped_store);
        let root_hash = jmt
            .get_root_hash(version)
            .map_err(|e| HotStuffError::StateTreeError(e.into()))?;

        shard_roots.insert(Shard::global(), root_hash);
    } else {
        shard_roots.insert(Shard::global(), SPARSE_MERKLE_PLACEHOLDER_HASH);
    }

    for shard in shard_group.shard_iter() {
        let Some(version) = tx.state_tree_versions_get_latest(shard)? else {
            // At v0 there have been no state changes
            shard_roots.insert(shard, SPARSE_MERKLE_PLACEHOLDER_HASH);
            continue;
        };

        let scoped_store = ShardScopedTreeStoreReader::new(tx, shard);
        let jmt = JellyfishMerkleTree::new(&scoped_store);
        let root_hash = jmt
            .get_root_hash(version)
            .map_err(|e| HotStuffError::StateTreeError(e.into()))?;

        shard_roots.insert(shard, root_hash);
    }
    let checkpoint = EpochCheckpoint::new(commit_block, qcs, shard_roots);
    let calculated_mr = checkpoint.compute_state_merkle_root()?;
    if calculated_mr != checkpoint.block().state_merkle_root() {
        return Err(HotStuffError::InvariantError(format!(
            "Epoch checkpoint state merkle root mismatch. Expected: {}, Calculated: {}, block: {}",
            checkpoint.block().state_merkle_root(),
            calculated_mr,
            checkpoint.block()
        )));
    }

    Ok(checkpoint)
}

pub(crate) fn filter_diff_for_committee(committee_info: &CommitteeInfo, diff: &SubstateDiff) -> SubstateDiff {
    let mut filtered_diff = SubstateDiff::new();
    filtered_diff
        .extend_up(
            diff.up_iter()
                .filter(|(id, _)| committee_info.includes_substate_id(id))
                .cloned(),
        )
        .extend_down(
            diff.down_iter()
                .filter(|(id, _)| committee_info.includes_substate_id(id))
                .cloned(),
        )
        .set_once_fee_withdrawals(
            diff.validator_fee_withdrawals()
                .iter()
                .filter(|f| committee_info.includes_substate_id(&f.address.into()))
                .cloned()
                .collect(),
        );
    filtered_diff
}

pub(crate) fn get_next_block_height_and_leader<
    'a,
    TTx: StateStoreReadTransaction,
    TLeaderStrategy: LeaderStrategy<TAddr>,
    TAddr: NodeAddressable,
>(
    tx: &TTx,
    committee: &'a Committee<TAddr>,
    leader_strategy: &TLeaderStrategy,
    block_id: &BlockId,
    height: NodeHeight,
) -> Result<(NodeHeight, &'a TAddr, usize), HotStuffError> {
    let mut num_skipped = 0;
    let mut next_height = height;
    let (mut leader_addr, mut leader_pk) = leader_strategy.get_leader_for_next_height(committee, next_height);

    while ValidatorConsensusStats::is_node_evicted(tx, block_id, leader_pk)? {
        debug!(target: LOG_TARGET, "Validator {} evicted for next height {}. Checking next validator", leader_addr, next_height + NodeHeight(1));
        next_height += NodeHeight(1);
        num_skipped += 1;
        let (addr, pk) = leader_strategy.get_leader_for_next_height(committee, next_height);
        leader_addr = addr;
        leader_pk = pk;
    }
    debug!(target: LOG_TARGET, "Validator {} selected as next leader at height {}", leader_addr, next_height);

    Ok((next_height, leader_addr, num_skipped))
}

pub fn apply_leader_fee_to_substate_store<TStore: StateStore>(
    store: &mut PendingSubstateStore<TStore>,
    claim_public_key_bytes: &RistrettoPublicKeyBytes,
    shard: Shard,
    num_preshards: NumPreshards,
    total_leader_fee: Amount,
) -> Result<(), HotStuffError> {
    // Basic defensive checks
    assert!(
        total_leader_fee.is_positive(),
        "apply_leader_fee_to_substate_store: total_leader_fee ({total_leader_fee}) must be positive"
    );
    if total_leader_fee.is_zero() {
        // Nothing to do
        return Ok(());
    }

    let fee_substate_id = derive_fee_pool_address(claim_public_key_bytes, num_preshards, shard);
    store.update_in_place(
        &fee_substate_id.into(),
        |value_mut| {
            debug!(target: LOG_TARGET, "🪙 Deposit leader fee {total_leader_fee} into fee pool {fee_substate_id}");
            let substate_type = SubstateType::from(&*value_mut);
            let pool = value_mut.as_validator_fee_pool_mut().ok_or_else(|| {
                HotStuffError::InvariantError(format!(
                    "Unexpected substate type {} was found at address {}",
                    substate_type, fee_substate_id
                ))
            })?;
            if !pool.deposit_direct(total_leader_fee) {
                return Err(HotStuffError::InvariantError(format!(
                    "Failed to deposit leader fee {total_leader_fee} into fee pool {fee_substate_id}"
                )));
            }

            Ok(())
        },
        |_| Ok(ValidatorFeePool::new(*claim_public_key_bytes, total_leader_fee).into()),
    )?;

    Ok(())
}
