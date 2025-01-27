//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    ops::{ControlFlow, Deref},
};

use indexmap::IndexMap;
use log::*;
use tari_common::configuration::Network;
use tari_common_types::types::{FixedHash, PublicKey};
use tari_crypto::tari_utilities::ByteArray;
use tari_dan_common_types::{
    committee::{Committee, CommitteeInfo},
    derive_fee_pool_address,
    optional::Optional,
    shard::Shard,
    substate_type::SubstateType,
    Epoch,
    NodeAddressable,
    NodeHeight,
    NumPreshards,
    ShardGroup,
    VersionedSubstateId,
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
        VersionedStateHashTreeDiff,
    },
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};
use tari_engine_types::{
    substate::{Substate, SubstateDiff, SubstateId},
    template_models::Amount,
    vn_fee_pool::ValidatorFeePool,
};
use tari_state_tree::{JellyfishMerkleTree, StateTreeError};

use crate::{
    hotstuff::{
        substate_store::{PendingSubstateStore, ShardScopedTreeStoreReader, ShardedStateTree},
        HotStuffError,
    },
    traits::{LeaderStrategy, WriteableSubstateStore},
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
    parent_base_layer_block_height: u64,
    parent_base_layer_block_hash: FixedHash,
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
        parent_base_layer_block_height,
        parent_base_layer_block_hash,
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
    parent_base_layer_block_height: u64,
    parent_base_layer_block_hash: FixedHash,
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
        parent_base_layer_block_height,
        parent_base_layer_block_hash,
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
        justify_block.base_layer_block_height(),
        *justify_block.base_layer_block_hash(),
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
    parent_base_layer_block_height: u64,
    parent_base_layer_block_hash: FixedHash,
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
            leader.clone(),
            current_height,
            *qc.id(),
            epoch,
            shard_group,
            parent_merkle_root,
            parent_timestamp,
            parent_base_layer_block_height,
            parent_base_layer_block_hash,
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
) -> Result<(FixedHash, IndexMap<Shard, VersionedStateHashTreeDiff>), StateTreeError> {
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

pub(crate) fn create_epoch_checkpoint<TTx>(
    tx: &mut TTx,
    epoch: Epoch,
    shard_group: ShardGroup,
) -> Result<EpochCheckpoint, HotStuffError>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
{
    // Get the last 3 blocks in the previous epoch. These blocks should end the epoch.
    let mut blocks = Block::get_last_n_in_epoch(&**tx, 3, epoch)?;
    if blocks.is_empty() {
        return Err(HotStuffError::StorageError(StorageError::NotFound {
            item: "Block::get_last_n_in_epoch",
            key: epoch.to_string(),
        }));
    }

    let commit_block = blocks.pop().unwrap();
    let qcs = blocks.into_iter().map(|b| b.into_justify()).collect();

    // Fetch the state roots of the shards in the shard group
    let mut shard_roots = IndexMap::with_capacity(shard_group.len());
    for shard in shard_group.shard_iter() {
        let Some(version) = tx.state_tree_versions_get_latest(shard)? else {
            // At v0 there have been no state changes
            continue;
        };

        let scoped_store = ShardScopedTreeStoreReader::new(&**tx, shard);
        let jmt = JellyfishMerkleTree::new(&scoped_store);
        let root_hash = jmt
            .get_root_hash(version)
            .map_err(|e| HotStuffError::StateTreeError(e.into()))?;

        shard_roots.insert(shard, root_hash);
    }
    let checkpoint = EpochCheckpoint::new(commit_block, qcs, shard_roots);
    checkpoint.save(tx)?;

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

pub fn to_public_key_bytes(public_key: &PublicKey) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf.copy_from_slice(public_key.as_bytes());
    buf
}

pub fn apply_leader_fee_to_substate_store<TStore: StateStore>(
    store: &mut PendingSubstateStore<TStore>,
    claim_public_key_bytes: [u8; 32],
    shard: Shard,
    num_preshards: NumPreshards,
    total_leader_fee: Amount,
) -> Result<(), HotStuffError> {
    let fee_substate_id = derive_fee_pool_address(claim_public_key_bytes, num_preshards, shard);

    let substate_id = SubstateId::from(fee_substate_id);
    let (next_amount, next_version) = if let Some(latest) = store.get_latest_change(&substate_id).optional()? {
        match latest {
            SubstateChange::Up { id, substate, .. } => {
                let version = id.version();
                store.put(SubstateChange::Down {
                    id,
                    shard,
                    transaction_id: Default::default(),
                })?;
                let pool = substate.substate_value().as_validator_fee_pool().ok_or_else(|| {
                    HotStuffError::InvariantError(format!(
                        "Unexpected substate type {} was found at address {}",
                        SubstateType::from(substate.substate_value()),
                        substate_id
                    ))
                })?;
                (pool.amount + total_leader_fee, version + 1)
            },
            // If the latest fee pool is a Down, it was previously claimed
            SubstateChange::Down { id, .. } => (Amount::zero(), id.version() + 1),
        }
    } else {
        (Amount::zero(), 0)
    };

    store.put(SubstateChange::Up {
        id: VersionedSubstateId::new(fee_substate_id, next_version),
        shard,
        transaction_id: Default::default(),
        substate: Substate::new(0, ValidatorFeePool::new(claim_public_key_bytes.into(), next_amount)),
    })?;

    Ok(())
}
