//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus::{
    hotstuff::{ConsensusCurrentState, HotstuffEvent},
    messages::HotstuffMessage,
};
use tari_dan_common_types::{optional::Optional, NodeHeight, ShardGroup, SubstateAddress};
use tari_dan_storage::{
    consensus_models::{BlockId, LeafBlock, TransactionRecord},
    StateStore,
    StateStoreReadTransaction,
};
use tari_state_store_sqlite::SqliteStateStore;
use tari_transaction::{Transaction, TransactionId};
use tokio::{
    sync::{broadcast, mpsc, watch},
    task::JoinHandle,
};

use crate::support::{
    address::TestAddress,
    epoch_manager::TestEpochManager,
    executions_store::TestExecutionSpecStore,
    ValidatorBuilder,
};

pub struct ValidatorChannels {
    pub address: TestAddress,
    pub shard_group: ShardGroup,
    pub num_committees: u32,
    pub state_store: SqliteStateStore<TestAddress>,

    pub tx_new_transactions: mpsc::Sender<(Transaction, usize)>,
    pub tx_hs_message: mpsc::Sender<(TestAddress, HotstuffMessage)>,
    pub rx_broadcast: mpsc::Receiver<(Vec<TestAddress>, HotstuffMessage)>,
    pub rx_leader: mpsc::Receiver<(TestAddress, HotstuffMessage)>,
}

pub struct Validator {
    pub address: TestAddress,
    pub _shard_address: SubstateAddress,
    pub shard_group: ShardGroup,
    pub num_committees: u32,

    pub state_store: SqliteStateStore<TestAddress>,
    pub transaction_executions: TestExecutionSpecStore,
    pub epoch_manager: TestEpochManager,
    pub events: broadcast::Receiver<HotstuffEvent>,
    pub current_state_machine_state: watch::Receiver<ConsensusCurrentState>,

    pub handle: JoinHandle<()>,
}

impl Validator {
    pub fn builder() -> ValidatorBuilder {
        ValidatorBuilder::new()
    }

    pub fn state_store(&self) -> &SqliteStateStore<TestAddress> {
        &self.state_store
    }

    pub fn epoch_manager(&self) -> &TestEpochManager {
        &self.epoch_manager
    }

    pub fn get_transaction_pool_count(&self) -> usize {
        self.state_store
            .with_read_tx(|tx| tx.transaction_pool_count(None, None, None, false))
            .unwrap()
    }

    pub fn current_state_machine_state(&self) -> ConsensusCurrentState {
        *self.current_state_machine_state.borrow()
    }

    pub fn get_leaf_block(&self) -> LeafBlock {
        let epoch = self.epoch_manager.get_current_epoch();
        self.state_store
            .with_read_tx(|tx| LeafBlock::get(tx, epoch))
            .optional()
            .unwrap()
            .unwrap_or_else(|| LeafBlock {
                block_id: BlockId::zero(),
                height: NodeHeight::zero(),
                epoch,
            })
    }

    pub fn has_committed_substates(&self, tx_id: &TransactionId) -> bool {
        let tx = self.state_store().create_read_tx().unwrap();
        tx.substates_exists_for_transaction(tx_id).unwrap()
    }

    pub fn get_transaction(&self, transaction_id: &TransactionId) -> TransactionRecord {
        self.state_store
            .with_read_tx(|tx| TransactionRecord::get(tx, transaction_id))
            .unwrap()
    }
}
