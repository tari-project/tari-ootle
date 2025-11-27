//  Copyright 2023, The Tari Project
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

pub(crate) mod error;

use tari_epoch_manager::EpochManagerReader;
use tari_indexer_client::types::TransactionEntry;
use tari_ootle_common_types::{optional::Optional, NodeAddressable, SubstateRequirementRef, ToSubstateAddress};
use tari_transaction::{Transaction, TransactionId};
use tari_validator_node_rpc::client::{
    SubstateResult,
    TransactionResultStatus,
    ValidatorNodeClientFactory,
    ValidatorNodeRpcClient,
};

use crate::{
    network_client::TariNetworkClient,
    store::{IndexerStore, IndexerStoreReadTransaction, IndexerStoreWriteTransaction},
    transaction_manager::error::TransactionManagerError,
};

#[derive(Debug, Clone)]
pub struct TransactionManager<TEpochManager, TClientFactory, TStore> {
    network_client: TariNetworkClient<TEpochManager, TClientFactory>,
    store: TStore,
}

impl<TEpochManager, TClientFactory, TAddr, TStore> TransactionManager<TEpochManager, TClientFactory, TStore>
where
    TAddr: NodeAddressable + 'static,
    TEpochManager: EpochManagerReader<Addr = TAddr> + 'static,
    TClientFactory: ValidatorNodeClientFactory<TAddr> + 'static,
    TStore: IndexerStore,
{
    pub fn new(network_client: TariNetworkClient<TEpochManager, TClientFactory>, store: TStore) -> Self {
        Self { network_client, store }
    }

    pub async fn submit_transaction(&self, transaction: Transaction) -> Result<TransactionId, TransactionManagerError> {
        if !transaction.verify_all_signatures() {
            // DEV note: If signatures are invalid here, this is probably an issue
            // with the JSON decoding (crates/engine_types/src/argument_parser.rs)
            return Err(TransactionManagerError::InvalidTransaction {
                transaction_id: transaction.calculate_id(),
                details: "Transaction has one or more invalid signature(s)".to_string(),
            });
        }
        self.store
            .with_write_tx(|tx| tx.insert_or_ignore_transaction(&transaction))?;
        let id = self.network_client.submit_transaction(transaction).await?;
        Ok(id)
    }

    pub async fn get_transaction_result(
        &self,
        transaction_id: TransactionId,
    ) -> Result<TransactionResultStatus, TransactionManagerError> {
        let transaction_substate_address = transaction_id.to_substate_address();
        self.network_client
            .try_single_with_committee(transaction_substate_address, |mut client| async move {
                client.get_finalized_transaction_result(transaction_id).await.optional()
            })
            .await?
            .ok_or_else(|| TransactionManagerError::NotFound {
                entity: "Transaction result",
                key: transaction_id.to_string(),
            })
    }

    pub async fn get_substate_from_network(
        &self,
        substate_requirement: SubstateRequirementRef<'_>,
    ) -> Result<SubstateResult, TransactionManagerError> {
        let address = substate_requirement.or_zero_version().to_substate_address();
        let result = self
            .network_client
            .try_single_with_committee(address, |mut client| async move {
                client.get_substate(substate_requirement).await
            })
            .await?;
        Ok(result)
    }

    pub fn list_recent_transactions(
        &self,
        last_id: Option<TransactionId>,
        limit: usize,
    ) -> Result<Vec<TransactionEntry>, TransactionManagerError> {
        let transactions = self
            .store
            .with_read_tx(|tx| tx.list_recent_transactions(last_id, limit))?;
        Ok(transactions)
    }
}
