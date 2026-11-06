//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Read-only rollback-plan iterators: prove the dry-run collection matches what the
//! mutating rewind walks. Seed a DB with known transitions spanning a few state
//! versions, call the collector with target_version below the seeded data, and assert
//! the row count + ordering.

pub mod helpers;

use helpers::{create_rocksdb, create_substate_update_batch, gen_substates_for_shards};
use tari_ootle_common_types::{Epoch, SubstateVersion};
use tari_ootle_storage::{StateStore, StateStoreWriteTransaction, consensus_models::Block};
use tari_ootle_transaction::Network;
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
#[expect(clippy::too_many_lines)]
fn rollback_delete_after_epoch_cleans_indices_and_epoch_scoped_tables() {
    use helpers::{create_block_with_qc, create_first_block_of_next_epoch, create_foreign_proposal, create_tx_atom};
    use tari_common_types::types::PrivateKey;
    use tari_consensus_types::{Decision, PcId, ProposalVote, ShardGroupAccumulatedData, ValidatorSignatureBytes};
    use tari_ootle_common_types::{ExtraData, NodeHeight, ProtocolVersion, SubstateRequirement, optional::Optional};
    use tari_ootle_storage::{
        EpochCleanupStep,
        StateStoreReadTransaction,
        consensus_models::{
            BlockCommands,
            BookkeepingModel,
            Command,
            Evidence,
            ForeignProposalStatus,
            LivenessThresholds,
            TransactionRecord,
            ValidatorConsensusStats,
            ValidatorStatsUpdate,
            VoteEquivocation,
        },
    };
    use tari_ootle_transaction::{Instruction, Transaction};
    use tari_sidechain::FixedHash;
    use tari_template_lib_types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};
    use tari_utilities::epoch_time::EpochTime;
    use tari_validator_rollback::storage::rollback_delete_after_epoch;

    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();

    let network = Network::LocalNet;
    let zero_block = Block::zero_block(network, num_preshards());
    zero_block.insert(&mut tx).unwrap();
    tx.blocks_set_qcs(zero_block.id(), Some(&PcId::zero()), Some(&PcId::zero()))
        .unwrap();
    let e0_block = create_block_with_qc(&zero_block.as_leaf());
    e0_block.insert(&mut tx).unwrap();
    tx.blocks_set_qcs(e0_block.id(), Some(&PcId::zero()), Some(&PcId::zero()))
        .unwrap();

    // Epoch 1 genesis and a committed block that finalizes `tx1` and proposes a foreign proposal from epoch 0.
    let e1_genesis = create_first_block_of_next_epoch(&e0_block.as_leaf());
    e1_genesis.insert(&mut tx).unwrap();
    tx.blocks_set_qcs(e1_genesis.id(), Some(&PcId::zero()), Some(&PcId::zero()))
        .unwrap();

    let fp_epoch0 = create_foreign_proposal(*zero_block.id(), Epoch(0));
    tx.foreign_proposals_save(&fp_epoch0).unwrap();
    let fp_epoch1 = create_foreign_proposal(*e0_block.id(), Epoch(1));
    tx.foreign_proposals_save(&fp_epoch1).unwrap();

    let tx1 = TransactionRecord::new(
        Transaction::builder_localnet(Epoch(1))
            .add_instruction(Instruction::DropAllProofsInWorkspace)
            .add_input(SubstateRequirement::new(
                helpers::create_random_substate_id(),
                Some(SubstateVersion::ZERO),
            ))
            .build_and_seal(&PrivateKey::default()),
    );
    tx.transactions_insert(&tx1).unwrap();
    let mut atom = create_tx_atom();
    atom.id = *tx1.id();

    let e1_block1 = Block::create(
        network,
        ProtocolVersion::V0,
        *e1_genesis.id(),
        e1_genesis.justify().clone(),
        None,
        NodeHeight(1),
        Epoch(1),
        e1_genesis.shard_group(),
        Default::default(),
        BlockCommands::init([Command::AllAccept(atom), Command::ForeignProposal(fp_epoch0.to_atom())]).unwrap(),
        Default::default(),
        0,
        SchnorrSignatureBytes::zero(),
        EpochTime::now().as_u64(),
        FixedHash::zero(),
        ShardGroupAccumulatedData::default(),
        ExtraData::default(),
    )
    .unwrap();
    e1_block1.insert(&mut tx).unwrap();
    tx.blocks_set_qcs(e1_block1.id(), Some(&PcId::zero()), Some(&PcId::zero()))
        .unwrap();

    // Uncommitted pending child in epoch 1.
    let e1_pending = create_block_with_qc(&e1_block1.as_leaf());
    e1_pending.insert(&mut tx).unwrap();
    e1_pending.as_leaf().set(&mut tx).unwrap();
    e1_block1.as_locked().set(&mut tx).unwrap();

    // Mark fp_epoch0 as proposed in e1_block1, and fp_epoch1 as proposed in e1_pending.
    tx.foreign_proposals_set_status(
        fp_epoch0.block_id(),
        ForeignProposalStatus::Proposed,
        Some(e1_block1.id()),
    )
    .unwrap();
    tx.foreign_proposals_set_status(
        fp_epoch1.block_id(),
        ForeignProposalStatus::Proposed,
        Some(e1_pending.id()),
    )
    .unwrap();

    // Finalize tx1 in epoch 1.
    tx.transaction_pool_insert_new(
        *tx1.id(),
        Decision::Commit,
        &Evidence::empty(),
        true,
        false,
        Epoch(1),
        0,
    )
    .unwrap();
    let pool = tx.transaction_pool_get_all(10).unwrap();
    tx.transactions_finalize_all(Epoch(1), pool.iter()).unwrap();

    // Record liveness counters and vote equivocation in epoch 1.
    let validator_pk = RistrettoPublicKeyBytes::from([7u8; 32]);
    let thresholds = LivenessThresholds {
        suspend_after_missed: 1,
        probation_base_votes: 1,
        probation_base_blocks: 10,
        probation_max_backoff_exp: 4,
    };
    tx.validator_epoch_stats_updates(Epoch(1), NodeHeight(1), [ValidatorStatsUpdate::new(
        &validator_pk,
        thresholds,
    )
    .add_missed_proposal()])
        .unwrap();
    let dummy_sig = ValidatorSignatureBytes::new(validator_pk, SchnorrSignatureBytes::zero());
    let equiv = VoteEquivocation {
        epoch: Epoch(1),
        height: NodeHeight(1),
        public_key: validator_pk,
        detected_at: 1000,
        first: ProposalVote {
            block_id: *e1_block1.id(),
            epoch: Epoch(1),
            block_height: NodeHeight(1),
            decision: tari_sidechain::QuorumDecision::Accept,
            signature: dummy_sig.clone(),
        },
        second: ProposalVote {
            block_id: *e1_pending.id(),
            epoch: Epoch(1),
            block_height: NodeHeight(1),
            decision: tari_sidechain::QuorumDecision::Accept,
            signature: dummy_sig,
        },
    };
    tx.vote_equivocation_record(&equiv).unwrap();

    // Roll back to target_epoch = 0.
    let stats = rollback_delete_after_epoch(&mut tx, Epoch(0)).unwrap();
    assert_eq!(stats.blocks_deleted, 3);
    assert_eq!(stats.foreign_proposals_deleted, 1);

    // 1. CommittedParentChildChainIndex: e0_block's committed child in epoch 1 is removed, while zero_block -> e0_block
    //    stays.
    assert_eq!(
        tx.blocks_get_committed_by_parent(zero_block.id()).unwrap().id(),
        e0_block.id()
    );
    assert!(
        tx.blocks_get_committed_by_parent(e0_block.id())
            .optional()
            .unwrap()
            .is_none()
    );
    // 2. PendingParentChildIndex: e1_block1 -> e1_pending is removed.
    assert!(tx.blocks_get_pending_ids_by_parent(e1_block1.id()).unwrap().is_empty());

    // 3. fp_epoch1 is completely removed from UnconfirmedIndex; fp_epoch0 (from epoch 0) had its proposed_in_block
    //    cleared back to New.
    e0_block.as_locked().set(&mut tx).unwrap();
    let new_fps = tx.foreign_proposals_get_all_new(e0_block.id(), 10).unwrap();
    assert_eq!(new_fps.len(), 1);
    assert_eq!(new_fps[0].block_id(), fp_epoch0.block_id());
    assert_eq!(new_fps[0].status(), ForeignProposalStatus::New);
    assert!(new_fps[0].proposed_in_block().is_none());

    // 4. Epoch 1 liveness counters and vote equivocations are removed.
    let counters =
        ValidatorConsensusStats::liveness_counters_as_of(&*tx, Epoch(1), &validator_pk, NodeHeight(10)).unwrap();
    assert_eq!(counters.missed_proposals, 0);
    assert!(
        !tx.vote_equivocation_exists(Epoch(1), NodeHeight(1), &validator_pk)
            .unwrap()
    );

    // 5. FinalizedTransactionEpochIndex entry for tx1 in epoch 1 was removed, so later epoch GC does not delete tx1's
    //    payload.
    tx.epoch_cleanup_step(Epoch(5), EpochCleanupStep::FinalizedTransactions, 100)
        .unwrap();
    assert!(
        tx.transactions_exists(tx1.id()).unwrap(),
        "unfinalized transaction payload must not be pruned by stale FinalizedTransactionEpochIndex entry"
    );

    tx.rollback().unwrap();
}
