//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Read-only rollback-plan iterators: prove the dry-run collection matches what the
//! mutating rewind walks. Seed a DB with known transitions spanning a few state
//! versions, call the collector with target_version below the seeded data, and assert
//! the row count + ordering.

pub mod helpers;

use helpers::{create_rocksdb, create_substate_update_batch, gen_substates_for_shards};
use tari_ootle_common_types::{Epoch, SubstateVersion};
use tari_ootle_storage::{Ordering, StateStore, StateStoreWriteTransaction, consensus_models::Block};
use tari_ootle_transaction::Network;
use tari_state_store_rocksdb::column_families::foreign_proposal;
use tari_state_tree::Version;
use tari_validator_rollback::storage::{
    RewindTransitionKind,
    rollback_plan_collect_blocks,
    rollback_plan_collect_substates,
};

use crate::helpers::num_preshards;

#[test]
fn collect_substates_yields_reverse_application_order() {
    let (db, _tmp) = create_rocksdb();

    let mut tx = db.create_write_tx().unwrap();
    let zero_block = Block::zero_block(Network::LocalNet, num_preshards());
    zero_block.insert(&mut tx).unwrap();

    // Seed substate commits across multiple state versions on shard 1.
    for batch_idx in 0..3 {
        let substates = gen_substates_for_shards(Epoch::zero(), 1, batch_idx..(batch_idx + 2), SubstateVersion::ZERO)
            .collect::<Vec<_>>();
        let batch = create_substate_update_batch(Epoch::zero(), &substates);
        tx.substates_commit_batch(batch).unwrap();
    }
    tx.commit().unwrap();

    let rows = db
        .with_read_tx(|tx| rollback_plan_collect_substates(tx, tari_ootle_common_types::shard::Shard::from_u32(1), 0))
        .unwrap();

    // At minimum we expect some transitions; all on shard 1, epoch 0, and Up-reverted.
    assert!(!rows.is_empty(), "expected at least one transition, got none");
    for (idx, row) in rows.iter().enumerate() {
        assert_eq!(row.epoch, Epoch::zero(), "row {idx} has wrong epoch");
        assert_eq!(row.transition, RewindTransitionKind::UpReverted, "row {idx} wrong kind");
        assert_eq!(row.shard.as_u32(), 1, "row {idx} wrong shard");
    }
    // Rows are yielded in reverse state_version order — check monotonic non-increase.
    for window in rows.windows(2) {
        assert!(
            window[0].state_version >= window[1].state_version,
            "rows out of order: {} before {}",
            window[0].state_version,
            window[1].state_version,
        );
    }
}

#[test]
fn collect_substates_empty_when_target_version_above_everything() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();
    let zero_block = Block::zero_block(Network::LocalNet, num_preshards());
    zero_block.insert(&mut tx).unwrap();
    let substates = gen_substates_for_shards(Epoch::zero(), 1, 0..2, SubstateVersion::ZERO).collect::<Vec<_>>();
    let batch = create_substate_update_batch(Epoch::zero(), &substates);
    tx.substates_commit_batch(batch).unwrap();
    tx.commit().unwrap();

    let rows = db
        .with_read_tx(|tx| {
            rollback_plan_collect_substates(tx, tari_ootle_common_types::shard::Shard::from_u32(1), Version::MAX)
        })
        .unwrap();
    assert!(rows.is_empty());
}

#[test]
fn collect_blocks_empty_when_no_blocks_after_target_epoch() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();
    let zero_block = Block::zero_block(Network::LocalNet, num_preshards());
    zero_block.insert(&mut tx).unwrap();
    tx.commit().unwrap();

    // Only the genesis / epoch-0 block exists — rolling back to epoch 0 yields no blocks.
    let rows = db
        .with_read_tx(|tx| rollback_plan_collect_blocks(tx, Epoch::zero()))
        .unwrap();
    assert!(rows.is_empty(), "unexpected rows: {rows:?}");
}

#[test]
fn rollback_delete_after_epoch_cleans_foreign_proposal_indexes_and_restores_surviving() {
    use helpers::{create_block_with_qc, create_first_block_of_next_epoch, create_foreign_proposal};
    use tari_ootle_storage::{
        StateStoreReadTransaction,
        consensus_models::{BookkeepingModel, ForeignProposalStatus},
    };
    use tari_validator_rollback::storage::rollback_delete_after_epoch;

    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();
    let zero_block = Block::zero_block(Network::LocalNet, num_preshards());
    zero_block.insert(&mut tx).unwrap();
    zero_block.as_locked().set(&mut tx).unwrap();

    let epoch0_block = create_block_with_qc(&zero_block.as_leaf());
    epoch0_block.insert(&mut tx).unwrap();
    let epoch1_block = create_first_block_of_next_epoch(&epoch0_block.as_leaf());
    epoch1_block.insert(&mut tx).unwrap();

    // Surviving foreign proposal from epoch 0 that was confirmed in an epoch 1 block.
    let fp_epoch0 = create_foreign_proposal(*zero_block.id(), Epoch(0));
    tx.foreign_proposals_save(&fp_epoch0).unwrap();
    tx.foreign_proposals_set_status(
        fp_epoch0.block_id(),
        ForeignProposalStatus::Confirmed,
        Some(epoch1_block.id()),
    )
    .unwrap();

    // Unconfirmed foreign proposal from epoch 1 that must be deleted along with its UnconfirmedIndex entry.
    let fp_epoch1 = create_foreign_proposal(*epoch0_block.id(), Epoch(1));
    tx.foreign_proposals_save(&fp_epoch1).unwrap();

    let stats = rollback_delete_after_epoch(&mut tx, Epoch(0)).unwrap();
    assert_eq!(stats.blocks_deleted, 1);
    assert_eq!(stats.foreign_proposals_deleted, 1);

    // Re-set locked block at epoch 0 so `foreign_proposals_get_all_new` can query up to epoch 0.
    epoch0_block.as_locked().set(&mut tx).unwrap();
    tx.commit().unwrap();

    db.with_read_tx(|tx| {
        assert!(!tx.foreign_proposals_exists(fp_epoch1.block_id()).unwrap());

        // Only the restored epoch 0 proposal remains unconfirmed; the deleted epoch 1 entry is gone.
        let unconfirmed = tx
            .db()
            .cf(foreign_proposal::UnconfirmedIndex)
            .unwrap()
            .key_iterator(Ordering::Ascending, "test")
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(unconfirmed, vec![(Epoch(0), *fp_epoch0.block_id())]);

        let proposed_in_deleted_block = tx
            .db()
            .cf(foreign_proposal::ByProposedInBlockIndexQuery)
            .unwrap()
            .query_prefix_range_keys(Ordering::Ascending, epoch1_block.id())
            .unwrap();
        assert!(
            proposed_in_deleted_block.is_empty(),
            "dangling ProposedInBlockIndex entries: {proposed_in_deleted_block:?}"
        );

        let new_fps = tx.foreign_proposals_get_all_new(epoch0_block.id(), 10).unwrap();
        assert_eq!(new_fps.len(), 1);
        assert_eq!(new_fps[0].block_id(), fp_epoch0.block_id());
        assert_eq!(new_fps[0].status(), ForeignProposalStatus::New);
        assert_eq!(new_fps[0].proposed_in_block(), None);
        Ok::<_, tari_ootle_storage::StorageError>(())
    })
    .unwrap();
}

/// Two conflicting votes by `signer` at height 3 of `epoch`.
fn vote_equivocation_at(
    epoch: Epoch,
    signer: tari_template_lib_types::crypto::RistrettoPublicKeyBytes,
) -> tari_ootle_storage::consensus_models::VoteEquivocation {
    use tari_consensus_types::{BlockId, ProposalVote, ValidatorSignatureBytes};
    use tari_ootle_common_types::NodeHeight;
    use tari_ootle_storage::consensus_models::VoteEquivocation;
    use tari_sidechain::{FixedHash, QuorumDecision};
    use tari_template_lib_types::crypto::{Scalar32Bytes, SchnorrSignatureBytes};

    let vote = |nonce: u8, block: u8| ProposalVote {
        epoch,
        block_id: BlockId::new(FixedHash::new([block; 32])),
        block_height: NodeHeight(3),
        decision: QuorumDecision::Accept,
        signature: ValidatorSignatureBytes::new(
            signer,
            SchnorrSignatureBytes::new([nonce; 32].into(), Scalar32Bytes::zero()),
        ),
    };
    VoteEquivocation::from_conflicting_votes(epoch, NodeHeight(3), signer, vote(1, 0xA), vote(2, 0xB)).unwrap()
}

/// Rows keyed by epoch above the rollback target go, rows at or below it stay. A delete built on
/// the wrong key layout would match nothing and leave the rows behind, which this catches.
#[test]
fn rollback_delete_after_epoch_removes_finalized_index_liveness_and_equivocation_rows() {
    use helpers::{create_block_with_qc, create_first_block_of_next_epoch_with_command, create_tx_atom};
    use tari_consensus_types::Decision;
    use tari_ootle_common_types::NodeHeight;
    use tari_ootle_storage::{
        StateStoreReadTransaction,
        consensus_models::{BookkeepingModel, Command, Evidence, LivenessCounters, TransactionRecord},
    };
    use tari_state_store_rocksdb::column_families::{
        finalized_transaction,
        validator_liveness_log::ValidatorLivenessLogCf,
        vote_equivocation,
    };
    use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
    use tari_validator_rollback::storage::rollback_delete_after_epoch;

    let signer = RistrettoPublicKeyBytes::from_bytes(&[1; 32]).unwrap();

    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();
    let zero_block = Block::zero_block(Network::LocalNet, num_preshards());
    zero_block.insert(&mut tx).unwrap();
    zero_block.as_locked().set(&mut tx).unwrap();

    let epoch0_block = create_block_with_qc(&zero_block.as_leaf());
    epoch0_block.insert(&mut tx).unwrap();

    // A transaction finalized by the epoch 1 block that the rollback deletes.
    let atom = create_tx_atom();
    let epoch1_block =
        create_first_block_of_next_epoch_with_command(&epoch0_block.as_leaf(), Command::AllAccept(atom.clone()));
    epoch1_block.insert(&mut tx).unwrap();
    tx.transaction_pool_insert_new(atom.id, Decision::Commit, &Evidence::empty(), true, false, Epoch(1), 0)
        .unwrap();
    let pool = tx.transaction_pool_get_all(10).unwrap();
    tx.transactions_finalize_all(Epoch(1), pool.iter()).unwrap();
    let finalized_in_epoch1 = tx
        .db()
        .cf(finalized_transaction::ByEpochQuery)
        .unwrap()
        .query_prefix_range_keys(Ordering::Ascending, &Epoch(1))
        .unwrap();
    assert_eq!(finalized_in_epoch1, vec![(Epoch(1), atom.id)]);

    for (epoch, height) in [(Epoch(0), NodeHeight(2)), (Epoch(1), NodeHeight(3))] {
        tx.db()
            .cf(ValidatorLivenessLogCf)
            .unwrap()
            .put(&(epoch, signer, height), &LivenessCounters::default(), "test")
            .unwrap();
        assert!(
            tx.vote_equivocation_record(&vote_equivocation_at(epoch, signer))
                .unwrap()
        );
    }

    let stats = rollback_delete_after_epoch(&mut tx, Epoch(0)).unwrap();
    assert_eq!(stats.blocks_deleted, 1);
    tx.commit().unwrap();

    db.with_read_tx(|tx| {
        assert!(!TransactionRecord::is_record_finalized(tx, atom.id()).unwrap());
        let finalized_in_epoch1 = tx
            .db()
            .cf(finalized_transaction::ByEpochQuery)
            .unwrap()
            .query_prefix_range_keys(Ordering::Ascending, &Epoch(1))
            .unwrap();
        assert!(
            finalized_in_epoch1.is_empty(),
            "dangling finalized EpochIndex entries: {finalized_in_epoch1:?}"
        );

        let liveness = tx
            .db()
            .cf(ValidatorLivenessLogCf)
            .unwrap()
            .key_iterator(Ordering::Ascending, "test")
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(liveness, vec![(Epoch(0), signer, NodeHeight(2))]);

        let equivocations = tx
            .db()
            .cf(vote_equivocation::VoteEquivocationCf)
            .unwrap()
            .key_iterator(Ordering::Ascending, "test")
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(equivocations, vec![(Epoch(0), NodeHeight(3), signer)]);
        Ok::<_, tari_ootle_storage::StorageError>(())
    })
    .unwrap();
}
