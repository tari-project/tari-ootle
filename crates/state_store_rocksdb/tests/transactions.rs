//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod helpers;
use std::time::Duration;

use helpers::{assert_eq_debug, commit_chain, create_chain, create_random_substate_id, create_rocksdb, create_tx_atom};
use tari_common_types::types::{FixedHash, PrivateKey};
use tari_consensus_types::{Decision, PcId, ShardGroupAccumulatedData};
use tari_engine_types::{
    commit_result::{ExecuteResult, FinalizeResult, TransactionResult},
    fees::FeeBreakdown,
    substate::SubstateDiff,
};
use tari_ootle_common_types::{Epoch, ExtraData, Network, NodeHeight, SubstateRequirement};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    consensus_models::{
        Block,
        BlockTransactionExecution,
        BookkeepingModel,
        Command,
        Evidence,
        TransactionPoolStage,
        TransactionPoolStatusUpdate,
        TransactionRecord,
    },
};
use tari_ootle_transaction::{Instruction, Transaction};
use tari_template_lib::types::Hash32;
use tari_utilities::epoch_time::EpochTime;

mod confirm_all_transitions {
    use tari_template_lib_types::crypto::SchnorrSignatureBytes;

    use super::*;
    use crate::helpers::num_preshards;

    #[test]
    fn it_sets_pending_stage_to_stage_rocksdb() {
        let (db, _tmp) = create_rocksdb();
        it_sets_pending_stage_to_stage(db);
    }

    fn it_sets_pending_stage_to_stage(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();

        let atom1 = create_tx_atom();
        let atom2 = create_tx_atom();
        let atom3 = create_tx_atom();

        let network = Network::LocalNet;
        let zero_block = Block::zero_block(network, num_preshards());
        zero_block.insert(&mut tx).unwrap();
        tx.proposal_certificates_save(zero_block.justify()).unwrap();
        tx.blocks_set_qcs(zero_block.id(), Some(&PcId::zero()), Some(&PcId::zero()))
            .unwrap();

        let shard_group = zero_block.shard_group();

        let block1 = Block::create(
            network,
            *zero_block.id(),
            zero_block.justify().clone(),
            None,
            NodeHeight(1),
            Epoch(0),
            shard_group,
            Default::default(),
            // Need to have a command in, otherwise this block will not be included internally in the query because it
            // cannot cause a state change without any commands
            [Command::LocalPrepare(atom1.clone())].into_iter().collect(),
            Default::default(),
            Default::default(),
            SchnorrSignatureBytes::zero(),
            EpochTime::now().as_u64(),
            FixedHash::zero(),
            ShardGroupAccumulatedData::default(),
            ExtraData::default(),
        )
        .unwrap();
        block1.insert(&mut tx).unwrap();
        block1.as_locked().set(&mut tx).unwrap();
        block1.as_leaf().set(&mut tx).unwrap();

        tx.transaction_pool_insert_new(atom1.id, atom1.decision, &Evidence::empty(), true, false, None)
            .unwrap();
        tx.transaction_pool_insert_new(atom2.id, atom2.decision, &Evidence::empty(), true, false, None)
            .unwrap();
        tx.transaction_pool_insert_new(atom3.id, atom3.decision, &Evidence::empty(), true, false, None)
            .unwrap();
        let block_id = *block1.id();
        let transactions = tx.transaction_pool_get_all(1000).unwrap();
        let mut tx_1 = transactions.iter().find(|tx| *tx.id() == atom1.id).unwrap().clone();
        let mut tx_2 = transactions.iter().find(|tx| *tx.id() == atom2.id).unwrap().clone();
        let mut tx_3 = transactions.iter().find(|tx| *tx.id() == atom3.id).unwrap().clone();

        assert!(tx.transaction_pool_exists(&atom1.id).unwrap());
        assert!(tx.transaction_pool_exists(&atom2.id).unwrap());
        assert!(tx.transaction_pool_exists(&atom3.id).unwrap());

        tx_1.set_next_stage_and_readiness(TransactionPoolStage::LocalPrepared, shard_group)
            .unwrap();
        tx_1.set_next_stage_and_readiness(TransactionPoolStage::LocalPrepared, shard_group)
            .unwrap();
        tx_2.set_next_stage_and_readiness(TransactionPoolStage::LocalPrepared, shard_group)
            .unwrap();
        tx_3.set_next_stage_and_readiness(TransactionPoolStage::LocalPrepared, shard_group)
            .unwrap();

        tx.transaction_pool_add_pending_update(&block1.as_leaf(), &TransactionPoolStatusUpdate::new(tx_1, true))
            .unwrap();
        tx.transaction_pool_add_pending_update(&block1.as_leaf(), &TransactionPoolStatusUpdate::new(tx_2, true))
            .unwrap();
        tx.transaction_pool_add_pending_update(&block1.as_leaf(), &TransactionPoolStatusUpdate::new(tx_3, true))
            .unwrap();

        let rec = tx.transaction_pool_get_many_ready(10, &block_id).unwrap();
        assert_eq!(rec.len(), 3);

        let rec = tx.transaction_pool_get_for_blocks(&block_id, &atom1.id).unwrap();
        assert!(rec.committed_stage().is_new());
        assert!(rec.pending_stage().unwrap().is_local_prepared());

        let rec = tx.transaction_pool_get_for_blocks(&block_id, &atom2.id).unwrap();
        assert!(rec.committed_stage().is_new());
        assert!(rec.pending_stage().unwrap().is_local_prepared());

        tx.transaction_pool_confirm_all_transitions(&block1.as_leaf()).unwrap();

        let rec = tx.transaction_pool_get_for_blocks(&block_id, &atom1.id).unwrap();
        assert!(rec.committed_stage().is_local_prepared());
        assert!(rec.pending_stage().is_none());

        let rec = tx.transaction_pool_get_for_blocks(&block_id, &atom2.id).unwrap();
        assert_eq!(rec.committed_stage(), TransactionPoolStage::LocalPrepared);
        assert_eq!(rec.pending_stage(), None);

        let rec = tx.transaction_pool_get_for_blocks(&block_id, &atom3.id).unwrap();
        assert_eq!(rec.committed_stage(), TransactionPoolStage::LocalPrepared);
        assert_eq!(rec.pending_stage(), None);

        tx.rollback().unwrap();
    }
}

mod transaction_operations {

    use super::*;

    #[test]
    fn transaction_operations_rocksdb() {
        let (db, _tmp) = create_rocksdb();
        transaction_operations(db);
    }

    fn transaction_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();

        // transactions_insert
        let tx1 = TransactionRecord::new(
            Transaction::builder_localnet()
                .add_instruction(Instruction::DropAllProofsInWorkspace)
                .add_input(SubstateRequirement::new(create_random_substate_id(), Some(0)))
                .build_and_seal(&PrivateKey::default()),
        );
        tx.transactions_insert(&tx1).unwrap();
        let tx2 = TransactionRecord::new(
            Transaction::builder_localnet()
                .add_instruction(Instruction::DropAllProofsInWorkspace)
                .add_input(SubstateRequirement::new(create_random_substate_id(), Some(1)))
                .build_and_seal(&PrivateKey::default()),
        );
        tx.transactions_insert(&tx2).unwrap();
        let unexisting_tx = TransactionRecord::new(
            Transaction::builder_localnet()
                .add_instruction(Instruction::DropAllProofsInWorkspace)
                .add_input(SubstateRequirement::new(create_random_substate_id(), Some(2)))
                .build_and_seal(&PrivateKey::default()),
        );

        // transactions_get
        let res = tx.transactions_get(tx1.id()).unwrap();
        assert_eq_debug(&res, &tx1);
        let res = tx.transactions_get(tx2.id()).unwrap();
        assert_eq_debug(&res, &tx2);
        assert!(tx.transactions_get(unexisting_tx.id()).is_err());

        // transactions_exists
        let res = tx.transactions_exists(tx1.id()).unwrap();
        assert!(res);
        let res = tx.transactions_exists(tx2.id()).unwrap();
        assert!(res);
        let res = tx.transactions_exists(unexisting_tx.id()).unwrap();
        assert!(!res);

        // transactions_update
        let updated_tx = TransactionRecord::new(
            Transaction::builder_localnet()
                .add_instruction(Instruction::DropAllProofsInWorkspace)
                .add_input(SubstateRequirement::new(create_random_substate_id(), Some(3)))
                .build_and_seal(&PrivateKey::default()),
        );
        tx.transactions_insert(&updated_tx).unwrap();

        let res = tx.transactions_get(updated_tx.id()).unwrap();
        assert_eq_debug(&res, &updated_tx);

        // transactions_get_any
        let res = tx
            .transactions_get_any(vec![tx1.id(), tx2.id(), unexisting_tx.id()])
            .unwrap();
        assert_eq!(res.len(), 2);

        // transactions_get_paginated
        // let res = tx.transactions_get_paginated(10, 0, None).unwrap();
        // assert_eq!(res.len(), 3);

        tx.rollback().unwrap();
    }
}

mod transaction_execution_operations {
    use tari_engine_types::fees::FeeReceiptBuilder;

    use super::*;

    #[test]
    fn transaction_execution_operations_rocksdb() {
        let (db, _tmp) = create_rocksdb();
        transaction_execution_operations(db);
    }

    fn transaction_execution_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();

        // insert some transactions
        let tx1 = TransactionRecord::new(
            Transaction::builder_localnet()
                .add_instruction(Instruction::DropAllProofsInWorkspace)
                .add_input(SubstateRequirement::new(create_random_substate_id(), Some(0)))
                .build_and_seal(&PrivateKey::default()),
        );
        tx.transactions_insert(&tx1).unwrap();
        let tx2 = TransactionRecord::new(
            Transaction::builder_localnet()
                .add_instruction(Instruction::DropAllProofsInWorkspace)
                .add_input(SubstateRequirement::new(create_random_substate_id(), Some(1)))
                .build_and_seal(&PrivateKey::default()),
        );
        tx.transactions_insert(&tx2).unwrap();

        // insert blocks
        let chain = create_chain(10);
        commit_chain(&mut tx, &chain);

        let not_committed_block = chain[9].clone();
        // insert transaction executions
        let exec1 = BlockTransactionExecution::new(
            not_committed_block.as_leaf(),
            *tx1.id(),
            ExecuteResult {
                finalize: FinalizeResult::new(
                    Hash32::default(),
                    vec![],
                    vec![],
                    TransactionResult::Accept(SubstateDiff::new()),
                    FeeReceiptBuilder {
                        total_fee_payment: 0,
                        total_fees_paid: 0,
                        total_fee_overcharge: 0,
                        cost_breakdown: FeeBreakdown::default(),
                    }
                    .build(),
                ),
                execution_time: Duration::from_secs(1),
                execute_epoch: None,
            },
            vec![],
            vec![],
        );
        tx.block_transaction_executions_insert_or_ignore(&exec1).unwrap();

        let committed_block = chain[6].clone();
        // insert transaction executions
        let exec2 = BlockTransactionExecution::new(
            committed_block.as_leaf(),
            *tx2.id(),
            ExecuteResult {
                finalize: FinalizeResult::new(
                    Hash32::default(),
                    vec![],
                    vec![],
                    TransactionResult::Accept(SubstateDiff::new()),
                    FeeReceiptBuilder {
                        total_fee_payment: 0,
                        total_fees_paid: 0,
                        total_fee_overcharge: 0,
                        cost_breakdown: FeeBreakdown::default(),
                    }
                    .build(),
                ),
                execution_time: Duration::from_secs(1),
                execute_epoch: None,
            },
            vec![],
            vec![],
        );
        assert!(tx.block_transaction_executions_insert_or_ignore(&exec2).unwrap());

        // transaction_executions_get_pending_for_block
        let res = tx
            .block_transaction_executions_get_pending_for_block(tx1.id(), &not_committed_block.as_leaf())
            .unwrap();
        assert_eq_debug(&res, &exec1);

        // transactions_finalize_all
        tx.transaction_pool_insert_new(*tx1.id(), Decision::Commit, &Evidence::empty(), true, false, None)
            .unwrap();
        let transactions = tx.transaction_pool_get_all(1000).unwrap();
        assert_eq!(transactions.len(), 1);
        tx.transactions_finalize_all(transactions.iter()).unwrap();

        let rec = tx.transactions_get(tx1.id()).unwrap();
        assert!(rec.is_finalized(&*tx).unwrap(), "Transaction should be finalized");

        let pending = tx
            .block_transaction_executions_get_pending_for_block(tx2.id(), &not_committed_block.as_leaf())
            .unwrap();
        assert_eq!(*pending.transaction_id(), *tx2.id());

        // block_transaction_executions_lock_any_for_block
        tx.block_transaction_executions_lock_any_for_block(&not_committed_block.as_leaf())
            .unwrap();

        tx.rollback().unwrap();
    }
}
