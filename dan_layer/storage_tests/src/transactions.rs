//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause


use tari_common_types::types::FixedHash;
use tari_dan_common_types::{Epoch, NodeHeight};
use tari_dan_storage::{
    consensus_models::{Block, Command, TransactionPoolStage, TransactionPoolStatusUpdate},
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};

use tari_utilities::epoch_time::EpochTime;

mod confirm_all_transitions {
    use tari_dan_common_types::{ExtraData, NumPreshards, ShardGroup};
    use tari_dan_storage::consensus_models::Evidence;

    use crate::helper::{create_rocksdb, create_sqlite, create_tx_atom};

    use super::*;

    #[test]
    fn it_sets_pending_stage_to_stage_sqlite() {
        let db = create_sqlite();
        it_sets_pending_stage_to_stage(db);
    }

    #[test]
    fn it_sets_pending_stage_to_stage_rocksdb() {
        let db = create_rocksdb();
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
            [Command::Prepare(atom1.clone())].into_iter().collect(),
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
        
        tx.transaction_pool_insert_new(atom1.id, atom1.decision, &Evidence::empty(), true, false).unwrap();
        tx.transaction_pool_insert_new(atom2.id, atom2.decision, &Evidence::empty(), true, false).unwrap();
        tx.transaction_pool_insert_new(atom3.id, atom3.decision, &Evidence::empty(), true, false).unwrap();
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
       
        tx_1.set_next_stage(TransactionPoolStage::Prepared).unwrap();
        tx_1.set_next_stage(TransactionPoolStage::LocalPrepared).unwrap();

        tx_2.set_next_stage(TransactionPoolStage::Prepared).unwrap();
        tx_3.set_next_stage(TransactionPoolStage::Prepared).unwrap();

        tx.transaction_pool_add_pending_update(&block_id, &TransactionPoolStatusUpdate::new(tx_1, true))
            .unwrap();
        tx.transaction_pool_add_pending_update(&block_id, &TransactionPoolStatusUpdate::new(tx_2, true))
            .unwrap();
        tx.transaction_pool_add_pending_update(&block_id, &TransactionPoolStatusUpdate::new(tx_3, true))
            .unwrap();

       
        let rec = tx
            .transaction_pool_get_for_blocks(zero_block.id(), &block_id, &atom1.id)
            .unwrap();
        assert!(rec.committed_stage().is_new());
        assert!(rec.pending_stage().unwrap().is_local_prepared());

        let rec = tx
            .transaction_pool_get_for_blocks(zero_block.id(), &block_id, &atom2.id)
            .unwrap();
        assert!(rec.committed_stage().is_new());
        assert!(rec.pending_stage().unwrap().is_prepared());


        tx.transaction_pool_confirm_all_transitions(&block1.as_locked_block())
            .unwrap();
        
        let rec = tx
            .transaction_pool_get_for_blocks(zero_block.id(), &block_id, &atom1.id)
            .unwrap();
        assert!(rec.committed_stage().is_local_prepared());
        assert!(rec.pending_stage().is_none());

        let rec = tx
            .transaction_pool_get_for_blocks(zero_block.id(), &block_id, &atom2.id)
            .unwrap();
        assert_eq!(rec.committed_stage(), TransactionPoolStage::Prepared);
        assert_eq!(rec.pending_stage(), None);

        let rec = tx
            .transaction_pool_get_for_blocks(zero_block.id(), &block_id, &atom3.id)
            .unwrap();
        assert_eq!(rec.committed_stage(), TransactionPoolStage::Prepared);
        assert_eq!(rec.pending_stage(), None);

        tx.rollback().unwrap();
    }
}

mod transaction_operations {
    use tari_common_types::types::PrivateKey;
    use tari_dan_common_types::SubstateRequirement;
    use tari_dan_storage::consensus_models::TransactionRecord;
    use tari_engine_types::commit_result::RejectReason;
    use tari_transaction::{Instruction, Transaction};

    use crate::helper::{assert_eq_debug, create_random_substate_id, create_rocksdb, create_sqlite};

    use super::*;

    #[ignore]
    #[test]
    fn transaction_operations_sqlite() {
        let db = create_sqlite();
        transaction_operations(db);
    }

    #[test]
    fn transaction_operations_rocksdb() {
        let db = create_rocksdb();
        transaction_operations(db);
    }

    fn transaction_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();
        
        // transactions_insert
        let tx1 = TransactionRecord::new(
            Transaction::builder()
            .add_instruction(Instruction::DropAllProofsInWorkspace)
            .add_input(SubstateRequirement::new(create_random_substate_id(), Some(0)))
            .build_and_seal(&PrivateKey::default())
        );
        tx.transactions_insert(&tx1).unwrap();
        let tx2 = TransactionRecord::new(
            Transaction::builder()
            .add_instruction(Instruction::DropAllProofsInWorkspace)
            .add_input(SubstateRequirement::new(create_random_substate_id(), Some(1)))
            .build_and_seal(&PrivateKey::default())
        );
        tx.transactions_insert(&tx2).unwrap();
        let unexisting_tx = TransactionRecord::new(
            Transaction::builder()
            .add_instruction(Instruction::DropAllProofsInWorkspace)
            .add_input(SubstateRequirement::new(create_random_substate_id(), Some(2)))
            .build_and_seal(&PrivateKey::default())
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
            .build_and_seal(&PrivateKey::default())
        );
        tx.transactions_insert(&updated_tx).unwrap();

        let res = tx.transactions_get(updated_tx.id()).unwrap();
        assert_eq_debug(&res, &updated_tx);
        assert_eq!(res.abort_reason, None);

        updated_tx.set_abort_reason(RejectReason::Unknown);

        tx.transactions_update(&updated_tx).unwrap();
        let res = tx.transactions_get(updated_tx.id()).unwrap();
        assert_eq_debug(&res, &updated_tx);
        assert_eq!(res.abort_reason, Some(RejectReason::Unknown));

        // transactions_get_any
        let res = tx.transactions_get_any(vec![tx1.id(), tx2.id(), unexisting_tx.id()]).unwrap();
        assert_eq!(res.len(), 2);

        // transactions_get_paginated
        let res = tx.transactions_get_paginated(10, 0, None).unwrap();
        assert_eq!(res.len(), 3);

        // transactions_save_all
        let tx3 = TransactionRecord::new(
            Transaction::builder()
            .add_instruction(Instruction::DropAllProofsInWorkspace)
            .add_input(SubstateRequirement::new(create_random_substate_id(), Some(1)))
            .build_and_seal(&PrivateKey::default())
        );
        let tx4 = TransactionRecord::new(
            Transaction::builder()
            .add_instruction(Instruction::DropAllProofsInWorkspace)
            .add_input(SubstateRequirement::new(create_random_substate_id(), Some(1)))
            .build_and_seal(&PrivateKey::default())
        );
        tx.transactions_save_all(vec![&tx3, &tx4]).unwrap();    
        let res = tx.transactions_get_paginated(10, 0, None).unwrap();
        assert_eq!(res.len(), 5);

        tx.rollback().unwrap();
    }
}

mod transaction_execution_operations {
    use std::{process::id, time::Duration};

    use tari_common_types::types::PrivateKey;
    use tari_dan_common_types::{NumPreshards, SubstateRequirement};
    use tari_dan_storage::consensus_models::{BlockTransactionExecution, Evidence, TransactionRecord};
    use tari_engine_types::{commit_result::{ExecuteResult, FinalizeResult, TransactionResult}, fees::{FeeBreakdown, FeeReceipt}, substate::SubstateDiff};
    use tari_template_lib::{models::Amount, Hash};
    use tari_transaction::{Instruction, Transaction};

    use crate::helper::{assert_eq_debug, create_random_substate_id, create_rocksdb, create_sqlite};

    use super::*;

    #[ignore]
    #[test]
    fn transaction_execution_operations_sqlite() {
        let db = create_sqlite();
        transaction_execution_operations(db);
    }

    #[test]
    fn transaction_execution_operations_rocksdb() {
        let db = create_rocksdb();
        transaction_execution_operations(db);
    }

    fn transaction_execution_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();
        
        // insert some transactions
        let tx1 = TransactionRecord::new(
            Transaction::builder()
            .add_instruction(Instruction::DropAllProofsInWorkspace)
            .add_input(SubstateRequirement::new(create_random_substate_id(), Some(0)))
            .build_and_seal(&PrivateKey::default())
        );
        tx.transactions_insert(&tx1).unwrap();
        let tx2 = TransactionRecord::new(
            Transaction::builder()
            .add_instruction(Instruction::DropAllProofsInWorkspace)
            .add_input(SubstateRequirement::new(create_random_substate_id(), Some(1)))
            .build_and_seal(&PrivateKey::default())
        );
        tx.transactions_insert(&tx2).unwrap();

        // insert blocks
        let network = Default::default();
        let zero_block = Block::zero_block(network, NumPreshards::P64);
        zero_block.insert(&mut tx).unwrap();

        // insert transaction executions
        let exec1 = BlockTransactionExecution::new(
            *zero_block.id(),
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
                    }
                ),
                execution_time: Duration::new(1, 0),
            },
            vec![],
            vec![],
            None,
        );
        tx.transaction_executions_insert_or_ignore(&exec1).unwrap();

        // transaction_executions_get
        let res = tx.transaction_executions_get(tx1.id(), zero_block.id()).unwrap();
        assert_eq_debug(&res, &exec1);

        // transaction_executions_get_pending_for_block
        let res = tx.transaction_executions_get_pending_for_block(tx1.id(), zero_block.id()).unwrap();
        assert_eq_debug(&res, &exec1);

        // transactions_finalize_all
        tx.transaction_pool_insert_new(*tx1.id(), tx1.current_decision(), &Evidence::empty(), true, false).unwrap();
        let transactions = tx.transaction_pool_get_all().unwrap();
        tx.transactions_finalize_all(*zero_block.id(), transactions.iter()).unwrap();     

        // transaction_executions_remove_any_by_block_id
        tx.transaction_executions_remove_any_by_block_id(zero_block.id()).unwrap();

        tx.rollback().unwrap();
    }
}