//  Copyright 2024. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{
    collections::HashMap, fmt, marker::PhantomData, ops::Deref, sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex}, time::{Duration, Instant}
};

use log::log;
use rocksdb::{ColumnFamily, SingleThreaded, TransactionDB, TransactionDBOptions};
use serde::{de::DeserializeOwned, Serialize};
use tari_dan_common_types::NodeAddressable;
use tari_dan_storage::{consensus_models::{LastVoted, StateTransition}, StateStore, StorageError};

use crate::{model::{block::BlockModel, block_transaction_execution::BlockTransactionExecutionModel, foreign_proposal::ForeignProposalModel, last_voted::LastVotedModel, missing_transactions::MissingTransactionModel, model::RocksdbModel, quorum_certificate::QuorumCertificateModel, state_transition::StateTransitionModel, substate::SubstateModel}, reader::RocksDbStateStoreReadTransaction, writer::RocksDbStateStoreWriteTransaction};

const LOG_TARGET: &str = "tari::dan::storage::rocksdb::state_store";

pub struct RocksDbStateStore<TAddr> {
    db: Arc<TransactionDB>,
    _addr: PhantomData<TAddr>,
}

impl<TAddr> RocksDbStateStore<TAddr> {
    pub fn connect(path: &str) -> Result<Self, StorageError> {
        let mut options = rocksdb::Options::default();
        options.set_error_if_exists(false);
        options.create_if_missing(true);
        options.create_missing_column_families(true);

        let cf_names = [
            BlockModel::column_families(),
            BlockTransactionExecutionModel::column_families(),
            ForeignProposalModel::column_families(),
            LastVotedModel::column_families(),
            MissingTransactionModel::column_families(),
            QuorumCertificateModel::column_families(),
            StateTransitionModel::column_families(),
            SubstateModel::column_families(),
        ].concat();

        let db = TransactionDB::<SingleThreaded>::open_cf(&options, &TransactionDBOptions::default(), path, cf_names.clone())
            .map_err(|e| StorageError::ConnectionError { reason: e.into_string() })?;

        Ok(Self {
            db: Arc::new(db),
            _addr: PhantomData,
        })
    }
}

// Manually implement the Debug implementation because `RocksDbStateStore` does not implement the Debug trait
impl<TAddr> fmt::Debug for RocksDbStateStore<TAddr> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "RocksDbStateStore")
    }
}

impl<TAddr: NodeAddressable + Serialize + DeserializeOwned> StateStore for RocksDbStateStore<TAddr> {
    type Addr = TAddr;
    type ReadTransaction<'a>
        = RocksDbStateStoreReadTransaction<'a, Self::Addr>
    where TAddr: 'a;
    type WriteTransaction<'a>
        = RocksDbStateStoreWriteTransaction<'a, Self::Addr>
    where TAddr: 'a;

    fn create_read_tx(&self) -> Result<Self::ReadTransaction<'_>, StorageError> {
        let tx = self.db.transaction();
        Ok(RocksDbStateStoreReadTransaction::new(self.db.clone(), tx))
    }

    fn create_write_tx(&self) -> Result<Self::WriteTransaction<'_>, StorageError> {
        let timer = Instant::now();
        let tx = self.db.transaction();
        let tx = RocksDbStateStoreWriteTransaction::new(self.db.clone(), tx);
        let elapsed = timer.elapsed();
        let level = if elapsed > Duration::from_secs(1) {
            log::Level::Warn
        } else {
            log::Level::Trace
        };
        log!(
            target: LOG_TARGET,
            level,
            "Write transaction obtained in {:?}", elapsed
        );
        Ok(tx)
    }
}

impl<TAddr> Clone for RocksDbStateStore<TAddr> {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            _addr: PhantomData,
        }
    }
}
