//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod helpers;

use std::ops::Deref;

use helpers::{
    NETWORK,
    build_substate_record,
    create_rocksdb_with_opts,
    create_substate_update_batch,
    num_preshards,
    random_substate_id_for_shard,
    substate_id_seed,
};
use tari_common_types::types::FixedHash;
use tari_consensus_types::BlockId;
use tari_engine_types::ProtocolVersion;
use tari_ootle_common_types::{Epoch, ShardGroup, SubstateVersion, ToSubstateAddress, shard::Shard};
use tari_ootle_storage::{
    ShardScopedTreeStoreReader,
    ShardScopedTreeStoreWriter,
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    SubstateProofGenerator,
    TrustedStateRoot,
    consensus_models::{
        CommittedBlockProof,
        StateVersionProof,
        StateVersionProofSource,
        SubstateDestroyed,
        SubstateRecord,
        index_substate_down_proofs,
    },
    verify_substate_down_proof_against_roots,
};
use tari_sidechain::{SidechainBlockCommitProof, SidechainBlockHeader};
use tari_state_store_rocksdb::DatabaseOptions;
use tari_state_tree::{
    SPARSE_MERKLE_PLACEHOLDER_HASH,
    ShardGroupRootTree,
    SpreadPrefixStateTree,
    SubstateTreeChange,
    TreeHash,
    Version,
};

const EPOCH: Epoch = Epoch(1);

fn protocol_version() -> ProtocolVersion {
    ProtocolVersion::at(NETWORK, EPOCH)
}

/// The shard-group root tree over the store's committed state.
fn group_tree(tx: &impl StateStoreReadTransaction, shard_group: ShardGroup) -> ShardGroupRootTree {
    let states = shard_group.shard_iter_with_global().map(|shard| {
        let Some(version) = tx.state_tree_versions_get_latest(shard).unwrap() else {
            return (shard, SPARSE_MERKLE_PLACEHOLDER_HASH, 0);
        };
        let mut store = ShardScopedTreeStoreReader::new(tx, shard);
        let root = SpreadPrefixStateTree::new(&mut store).get_root_hash(version).unwrap();
        (shard, root, version)
    });
    ShardGroupRootTree::build(protocol_version(), states.collect::<Vec<_>>()).unwrap()
}

fn commit_proof(shard_group: ShardGroup, height: u64, root: TreeHash) -> CommittedBlockProof {
    let header = SidechainBlockHeader {
        network: NETWORK.as_byte(),
        protocol_version: protocol_version().as_u32(),
        parent_id: FixedHash::zero(),
        justify_id: FixedHash::zero(),
        height,
        epoch: EPOCH.as_u64(),
        epoch_hash: FixedHash::zero(),
        shard_group: tari_sidechain::ShardGroup {
            start: shard_group.start().as_u32(),
            end_inclusive: shard_group.end().as_u32(),
        },
        proposed_by: Default::default(),
        state_merkle_root: FixedHash::new(root.into_array()),
        command_merkle_root: FixedHash::zero(),
        transaction_merkle_root: None,
        signature: Default::default(),
        accumulated_data: Default::default(),
        metadata_hash: FixedHash::zero(),
    };
    CommittedBlockProof::new(SidechainBlockCommitProof {
        header,
        proof_elements: vec![],
    })
}

/// Writes `changes` to `shard`'s tree at `version` and commits the substate records, recording a down proof for each
/// substate the batch destroys, as a validator does when a block commits.
fn commit_version<TTx>(
    tx: &mut TTx,
    shard: Shard,
    version: Version,
    changes: Vec<SubstateTreeChange>,
    records: &[&SubstateRecord],
    commit_proof_for_block: impl FnMut(&TTx::Target, &BlockId) -> Option<Vec<u8>>,
) where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
{
    {
        let mut store = ShardScopedTreeStoreWriter::new(tx, shard);
        SpreadPrefixStateTree::new(&mut store)
            .batch_put_substate_changes((version > 1).then(|| version - 1), version, changes)
            .unwrap();
    }
    tx.state_tree_shard_versions_set(shard, version).unwrap();
    let batch = create_substate_update_batch(EPOCH, records.iter().copied());
    let downed = batch.downed();
    tx.substates_commit_batch(batch).unwrap();
    index_substate_down_proofs(tx, &downed, commit_proof_for_block).unwrap();
}

fn up(record: &SubstateRecord) -> SubstateTreeChange {
    SubstateTreeChange::Up {
        id: record.to_versioned_substate_id(),
        value_hash: *record.state_hash(),
    }
}

fn destroyed(record: &SubstateRecord, at_state_version: Version) -> SubstateRecord {
    let mut record = record.clone();
    record.destroyed = Some(SubstateDestroyed {
        at_epoch: EPOCH,
        at_state_version,
    });
    record
}

fn no_committed_blocks<T: ?Sized>(_: &T, _: &BlockId) -> Option<Vec<u8>> {
    None
}

#[test]
fn a_destroyed_substate_stays_provably_down_after_its_state_is_pruned() {
    let (db, _dir) = create_rocksdb_with_opts(DatabaseOptions::default().with_state_history_length(1));

    let target = build_substate_record(&substate_id_seed(1 << 24), SubstateVersion::ZERO, 1);
    let shard = target.created().in_shard;
    let shard_group = ShardGroup::new(shard, shard);
    let target_id = target.to_versioned_substate_id();
    let neighbour = build_substate_record(&random_substate_id_for_shard(shard), SubstateVersion::ZERO, 1);

    // v1: the target is created; this node holds a proof of the shard at v1, as a synced node does at a proof point.
    let r1_commit_proof = {
        let mut tx = db.create_write_tx().unwrap();
        commit_version(
            &mut tx,
            shard,
            1,
            vec![up(&target), up(&neighbour)],
            &[&target, &neighbour],
            no_committed_blocks,
        );
        let tree = group_tree(&*tx, shard_group);
        let commit_proof = commit_proof(shard_group, 2, tree.root());
        tx.state_version_proofs_insert(&StateVersionProof {
            shard,
            state_version: 1,
            source: StateVersionProofSource::Received {
                commit_proof: commit_proof.to_bytes(),
            },
            shard_root_proof: tree.get_proof(shard).unwrap().1,
        })
        .unwrap();
        tx.commit().unwrap();
        commit_proof
    };
    let r1 = TrustedStateRoot::from_commit_proof(&r1_commit_proof).unwrap();

    // v2: the target is destroyed and its next version created.
    let next = build_substate_record(target.substate_id(), SubstateVersion::new(1), 2);
    {
        let mut tx = db.create_write_tx().unwrap();
        commit_version(
            &mut tx,
            shard,
            2,
            vec![SubstateTreeChange::Down { id: target_id.clone() }, up(&next)],
            &[&destroyed(&target, 2), &next],
            no_committed_blocks,
        );
        tx.commit().unwrap();
    }

    let verify_at_latest = |height: u64| {
        let tx = db.create_read_tx().unwrap();
        let record = tx.substate_down_proofs_get(shard, &target_id).unwrap().unwrap();
        assert_eq!(record.state_version, 1);
        let r2_commit_proof = commit_proof(shard_group, height, group_tree(&tx, shard_group).root());
        let down = SubstateProofGenerator::new(&tx, shard_group, num_preshards(), protocol_version())
            .unwrap()
            .generate(&target_id)
            .unwrap()
            .unwrap();
        let proof = tari_bor::serde_codec::to_vec(&record.into_down_proof(down)).unwrap();
        verify_substate_down_proof_against_roots(
            &proof,
            target_id.substate_id(),
            target_id.version(),
            NETWORK,
            num_preshards(),
            &r1,
            &TrustedStateRoot::from_commit_proof(&r2_commit_proof).unwrap(),
        )
    };
    verify_at_latest(4).unwrap();

    // v3: the next version is destroyed. It was created at v2, a version this node holds no usable proof of: its
    // only candidate is a block this node can no longer build a commit proof for.
    {
        let mut tx = db.create_write_tx().unwrap();
        tx.state_version_proofs_insert(&StateVersionProof {
            shard,
            state_version: 2,
            source: StateVersionProofSource::Committed {
                block_id: BlockId::zero(),
            },
            shard_root_proof: group_tree(&*tx, shard_group).get_proof(shard).unwrap().1,
        })
        .unwrap();
        commit_version(
            &mut tx,
            shard,
            3,
            vec![SubstateTreeChange::Down {
                id: next.to_versioned_substate_id(),
            }],
            &[&destroyed(&next, 3)],
            no_committed_blocks,
        );
        tx.commit().unwrap();
    }
    {
        let tx = db.create_read_tx().unwrap();
        assert!(
            tx.substate_down_proofs_get(shard, &next.to_versioned_substate_id())
                .unwrap()
                .is_none()
        );
    }

    // Prune the tree nodes of v1 and the target's value.
    {
        let mut tx = db.create_write_tx().unwrap();
        assert!(tx.state_tree_nodes_clear_stale(num_preshards(), usize::MAX).unwrap() > 0);
        tx.substates_prune_downed_values(EPOCH, usize::MAX).unwrap();
        tx.commit().unwrap();
    }
    {
        let tx = db.create_read_tx().unwrap();
        let mut store = ShardScopedTreeStoreReader::new(&tx, shard);
        assert!(
            SpreadPrefixStateTree::new(&mut store).get_proof(1, &target_id).is_err(),
            "v1 is still in the tree"
        );
        let record = SubstateRecord::get(&tx, &target_id.to_substate_address()).unwrap();
        assert!(record.substate_value().is_none(), "the target's value was not pruned");
    }

    verify_at_latest(6).unwrap();
}

#[test]
fn a_substate_destroyed_with_a_committed_block_proof_is_recorded() {
    let (db, _dir) = create_rocksdb_with_opts(DatabaseOptions::default());

    let target = build_substate_record(&substate_id_seed(2 << 24), SubstateVersion::ZERO, 1);
    let shard = target.created().in_shard;
    let shard_group = ShardGroup::new(shard, shard);
    let target_id = target.to_versioned_substate_id();
    let neighbour = build_substate_record(&random_substate_id_for_shard(shard), SubstateVersion::ZERO, 1);

    let block_id = BlockId::from([7u8; 32]);
    let mut tx = db.create_write_tx().unwrap();
    commit_version(
        &mut tx,
        shard,
        1,
        vec![up(&target), up(&neighbour)],
        &[&target, &neighbour],
        no_committed_blocks,
    );
    let tree = group_tree(&*tx, shard_group);
    let r1_commit_proof = commit_proof(shard_group, 2, tree.root());
    tx.state_version_proofs_insert(&StateVersionProof {
        shard,
        state_version: 1,
        source: StateVersionProofSource::Committed { block_id },
        shard_root_proof: tree.get_proof(shard).unwrap().1,
    })
    .unwrap();

    let commit_proof_bytes = r1_commit_proof.to_bytes();
    commit_version(
        &mut tx,
        shard,
        2,
        vec![SubstateTreeChange::Down { id: target_id.clone() }],
        &[&destroyed(&target, 2)],
        |_, id| (*id == block_id).then(|| commit_proof_bytes.clone()),
    );

    let record = tx.substate_down_proofs_get(shard, &target_id).unwrap().unwrap();
    assert_eq!(record.state_version, 1);
    assert_eq!(record.commit_proof, r1_commit_proof.to_bytes());
    assert_eq!(record.value_hash, TreeHash::new(target.state_hash().into_array()));
    let r1 = TrustedStateRoot::from_commit_proof(&r1_commit_proof).unwrap();
    let up = record.into_down_proof(
        SubstateProofGenerator::new(&*tx, shard_group, num_preshards(), protocol_version())
            .unwrap()
            .generate(&target_id)
            .unwrap()
            .unwrap(),
    );
    up.up
        .verify_inclusion(
            tari_state_tree::jmt_hash_scheme(protocol_version()),
            protocol_version(),
            &TreeHash::new(r1.root.into_array()),
            num_preshards(),
            &target_id,
            &up.up_value_hash,
        )
        .unwrap();
}
