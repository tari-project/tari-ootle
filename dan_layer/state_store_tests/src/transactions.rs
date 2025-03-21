//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use tari_common_types::types::{FixedHash, PrivateKey};
use tari_dan_common_types::{Epoch, ExtraData, NodeHeight, NumPreshards, ShardGroup, SubstateRequirement};
use tari_dan_storage::{
    consensus_models::{
        Block,
        BlockTransactionExecution,
        Command,
        Evidence,
        TransactionPoolStage,
        TransactionPoolStatusUpdate,
        TransactionRecord,
    },
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};
use tari_engine_types::{
    commit_result::{ExecuteResult, FinalizeResult, RejectReason, TransactionResult},
    fees::{FeeBreakdown, FeeReceipt},
    substate::SubstateDiff,
};
use tari_template_lib::{models::Amount, Hash};
use tari_transaction::{Instruction, Transaction};
use tari_utilities::epoch_time::EpochTime;

use crate::helper::{assert_eq_debug, create_random_substate_id, create_rocksdb, create_sqlite, create_tx_atom};

mod confirm_all_transitions {
    use super::*;

    #[test]
    fn it_sets_pending_stage_to_stage_sqlite() {
        let db = create_sqlite();
        it_sets_pending_stage_to_stage(db);
    }

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

        let network = Default::default();
        let zero_block = Block::zero_block(network, NumPreshards::P64);
        zero_block.insert(&mut tx).unwrap();
        zero_block.justify().save(&mut tx).unwrap();
        tx.blocks_set_flags(zero_block.id(), Some(true), Some(true)).unwrap();

        let block1 = Block::create(
            network,
            *zero_block.id(),
            zero_block.justify().clone(),
            NodeHeight(1),
            Epoch(0),
            ShardGroup::all_shards(NumPreshards::P64),
            Default::default(),
            // Need to have a command in, otherwise this block will not be included internally in the query because it
            // cannot cause a state change without any commands
            [Command::LocalPrepare(atom1.clone())].into_iter().collect(),
            Default::default(),
            Default::default(),
            Default::default(),
            None,
            EpochTime::now().as_u64(),
            0,
            FixedHash::zero(),
            ExtraData::default(),
        )
        .unwrap();
        block1.insert(&mut tx).unwrap();
        block1.as_locked_block().set(&mut tx).unwrap();
        block1.as_leaf_block().set(&mut tx).unwrap();

        tx.transaction_pool_insert_new(atom1.id, atom1.decision, &Evidence::empty(), true, false)
            .unwrap();
        tx.transaction_pool_insert_new(atom2.id, atom2.decision, &Evidence::empty(), true, false)
            .unwrap();
        tx.transaction_pool_insert_new(atom3.id, atom3.decision, &Evidence::empty(), true, false)
            .unwrap();
        let block_id = *block1.id();
        let transactions = tx.transaction_pool_get_all().unwrap();
        let mut tx_1 = transactions
            .iter()
            .find(|tx| *tx.transaction_id() == atom1.id)
            .unwrap()
            .clone();
        let mut tx_2 = transactions
            .iter()
            .find(|tx| *tx.transaction_id() == atom2.id)
            .unwrap()
            .clone();
        let mut tx_3 = transactions
            .iter()
            .find(|tx| *tx.transaction_id() == atom3.id)
            .unwrap()
            .clone();

        assert!(tx.transaction_pool_exists(&atom1.id).unwrap());
        assert!(tx.transaction_pool_exists(&atom2.id).unwrap());
        assert!(tx.transaction_pool_exists(&atom3.id).unwrap());

        tx_1.set_next_stage(TransactionPoolStage::LocalPrepared).unwrap();
        tx_1.set_next_stage(TransactionPoolStage::LocalPrepared).unwrap();
        tx_2.set_next_stage(TransactionPoolStage::LocalPrepared).unwrap();
        tx_3.set_next_stage(TransactionPoolStage::LocalPrepared).unwrap();

        tx.transaction_pool_add_pending_update(&block1.as_leaf_block(), &TransactionPoolStatusUpdate::new(tx_1, true))
            .unwrap();
        tx.transaction_pool_add_pending_update(&block1.as_leaf_block(), &TransactionPoolStatusUpdate::new(tx_2, true))
            .unwrap();
        tx.transaction_pool_add_pending_update(&block1.as_leaf_block(), &TransactionPoolStatusUpdate::new(tx_3, true))
            .unwrap();

        let rec = tx.transaction_pool_get_many_ready(10, &block_id).unwrap();
        assert_eq!(rec.len(), 3);

        let rec = tx.transaction_pool_get_for_blocks(&block_id, &atom1.id).unwrap();
        assert!(rec.committed_stage().is_new());
        assert!(rec.pending_stage().unwrap().is_local_prepared());

        let rec = tx.transaction_pool_get_for_blocks(&block_id, &atom2.id).unwrap();
        assert!(rec.committed_stage().is_new());
        assert!(rec.pending_stage().unwrap().is_local_prepared());

        tx.transaction_pool_confirm_all_transitions(&block1.as_leaf_block())
            .unwrap();

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

    #[ignore]
    #[test]
    fn transaction_operations_sqlite() {
        let db = create_sqlite();
        transaction_operations(db);
    }

    #[test]
    fn transaction_operations_rocksdb() {
        let (db, _tmp) = create_rocksdb();
        transaction_operations(db);
    }

    fn transaction_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();

        // transactions_insert
        let tx1 = TransactionRecord::new(
            Transaction::builder()
                .add_instruction(Instruction::DropAllProofsInWorkspace)
                .add_input(SubstateRequirement::new(create_random_substate_id(), Some(0)))
                .build_and_seal(&PrivateKey::default()),
        );
        tx.transactions_insert(&tx1).unwrap();
        let tx2 = TransactionRecord::new(
            Transaction::builder()
                .add_instruction(Instruction::DropAllProofsInWorkspace)
                .add_input(SubstateRequirement::new(create_random_substate_id(), Some(1)))
                .build_and_seal(&PrivateKey::default()),
        );
        tx.transactions_insert(&tx2).unwrap();
        let unexisting_tx = TransactionRecord::new(
            Transaction::builder()
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
        let mut updated_tx = TransactionRecord::new(
            Transaction::builder()
                .add_instruction(Instruction::DropAllProofsInWorkspace)
                .add_input(SubstateRequirement::new(create_random_substate_id(), Some(3)))
                .build_and_seal(&PrivateKey::default()),
        );
        tx.transactions_insert(&updated_tx).unwrap();

        let res = tx.transactions_get(updated_tx.id()).unwrap();
        assert_eq_debug(&res, &updated_tx);
        assert_eq!(res.abort_reason, None);

        updated_tx.abort_reason = Some(RejectReason::Unknown);

        // tx.transactions_update(&updated_tx).unwrap();
        // let res = tx.transactions_get(updated_tx.id()).unwrap();
        // assert_eq_debug(&res, &updated_tx);
        // assert_eq!(res.abort_reason, Some(RejectReason::Unknown));

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
    use tari_dan_common_types::optional::Optional;

    use super::*;
    use crate::helper::{commit_chain, create_chain};

    #[test]
    fn transaction_execution_operations_sqlite() {
        let db = create_sqlite();
        transaction_execution_operations(db);
    }

    #[test]
    fn transaction_execution_operations_rocksdb() {
        let (db, _tmp) = create_rocksdb();
        transaction_execution_operations(db);
    }

    fn transaction_execution_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();

        // insert some transactions
        let tx1 = TransactionRecord::new(
            Transaction::builder()
                .add_instruction(Instruction::DropAllProofsInWorkspace)
                .add_input(SubstateRequirement::new(create_random_substate_id(), Some(0)))
                .build_and_seal(&PrivateKey::default()),
        );
        tx.transactions_insert(&tx1).unwrap();
        let tx2 = TransactionRecord::new(
            Transaction::builder()
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
            *not_committed_block.id(),
            *tx1.id(),
            ExecuteResult {
                finalize: FinalizeResult::new(
                    Hash::default(),
                    vec![],
                    vec![],
                    TransactionResult::Accept(SubstateDiff::new()),
                    FeeReceipt {
                        total_fee_payment: Amount(0),
                        total_fees_paid: Amount(0),
                        cost_breakdown: FeeBreakdown::default(),
                    },
                ),
                execution_time: Duration::from_secs(1),
            },
            vec![],
            vec![],
            None,
        );
        tx.transaction_executions_insert_or_ignore(&exec1).unwrap();

        let committed_block = chain[6].clone();
        // insert transaction executions
        let exec2 = BlockTransactionExecution::new(
            *committed_block.id(),
            *tx2.id(),
            ExecuteResult {
                finalize: FinalizeResult::new(
                    Hash::default(),
                    vec![],
                    vec![],
                    TransactionResult::Accept(SubstateDiff::new()),
                    FeeReceipt {
                        total_fee_payment: Amount(0),
                        total_fees_paid: Amount(0),
                        cost_breakdown: FeeBreakdown::default(),
                    },
                ),
                execution_time: Duration::from_secs(1),
            },
            vec![],
            vec![],
            None,
        );
        tx.transaction_executions_insert_or_ignore(&exec2).unwrap();

        // transaction_executions_get
        let res = tx
            .transaction_executions_get(tx1.id(), not_committed_block.id())
            .unwrap();
        assert_eq_debug(&res, &exec1);

        // transaction_executions_get_pending_for_block
        let res = tx
            .transaction_executions_get_pending_for_block(tx1.id(), not_committed_block.id())
            .unwrap();
        assert_eq_debug(&res, &exec1);

        // transactions_finalize_all
        tx.transaction_pool_insert_new(*tx1.id(), tx1.current_decision(), &Evidence::empty(), true, false)
            .unwrap();
        let transactions = tx.transaction_pool_get_all().unwrap();
        assert_eq!(transactions.len(), 1);
        tx.transactions_finalize_all(*not_committed_block.id(), transactions.iter())
            .unwrap();

        let rec = tx.transactions_get(tx1.id()).unwrap();
        assert!(rec.is_finalized(), "Transaction should be finalized");

        let pending = tx
            .transaction_executions_get_pending_for_block(tx1.id(), not_committed_block.id())
            .optional()
            .unwrap();
        assert!(pending.is_none());

        let pending = tx
            .transaction_executions_get_pending_for_block(tx2.id(), not_committed_block.id())
            .unwrap();
        assert_eq!(pending.execution.transaction_id, *tx2.id());

        // transaction_executions_remove_any_by_block_id
        tx.transaction_executions_remove_any_by_block_id(not_committed_block.id())
            .unwrap();

        tx.rollback().unwrap();
    }
}
