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

use std::{iter, sync::Arc};

use tari_dan_common_types::{
    optional::{IsNotFoundError, Optional},
    NodeAddressable,
    SubstateRequirement,
    ToSubstateAddress,
};
use tari_epoch_manager::EpochManagerReader;
use tari_indexer_lib::{
    substate_cache::SubstateCache,
    substate_scanner::SubstateScanner,
    transaction_autofiller::TransactionAutofiller,
};
use tari_transaction::{Transaction, TransactionId};
use tari_validator_node_rpc::client::{
    SubstateResult,
    TransactionResultStatus,
    ValidatorNodeClientFactory,
    ValidatorNodeRpcClient,
};

use crate::{network_client::TariNetworkClient, transaction_manager::error::TransactionManagerError};

pub struct TransactionManager<TEpochManager, TClientFactory, TSubstateCache> {
    network_client: TariNetworkClient<TEpochManager, TClientFactory>,
    transaction_autofiller: TransactionAutofiller<TEpochManager, TClientFactory, TSubstateCache>,
}

impl<TEpochManager, TClientFactory, TAddr, TSubstateCache>
    TransactionManager<TEpochManager, TClientFactory, TSubstateCache>
where
    TAddr: NodeAddressable + 'static,
    TEpochManager: EpochManagerReader<Addr = TAddr> + 'static,
    TClientFactory: ValidatorNodeClientFactory<TAddr> + 'static,
    <TClientFactory::Client as ValidatorNodeRpcClient<TAddr>>::Error: IsNotFoundError + 'static,
    TSubstateCache: SubstateCache + 'static,
{
    pub fn new(
        network_client: TariNetworkClient<TEpochManager, TClientFactory>,
        substate_scanner: Arc<SubstateScanner<TEpochManager, TClientFactory, TSubstateCache>>,
    ) -> Self {
        Self {
            network_client,
            transaction_autofiller: TransactionAutofiller::new(substate_scanner),
        }
    }

    pub async fn submit_transaction(&self, transaction: Transaction) -> Result<TransactionId, TransactionManagerError> {
        let id = self.network_client.submit_transaction(transaction).await?;
        Ok(id)
    }

    pub async fn autofill_transaction(
        &self,
        transaction: Transaction,
        required_substates: Vec<SubstateRequirement>,
    ) -> Result<Transaction, TransactionManagerError> {
        let (transaction, _) = self
            .transaction_autofiller
            .autofill_transaction(transaction, required_substates)
            .await?;
        Ok(transaction)
    }

    pub async fn get_transaction_result(
        &self,
        transaction_id: TransactionId,
    ) -> Result<TransactionResultStatus, TransactionManagerError> {
        let transaction_substate_address = transaction_id.to_substate_address();
        self.network_client
            .try_with_committee(iter::once(transaction_substate_address), 1, |mut client| async move {
                client.get_finalized_transaction_result(transaction_id).await.optional()
            })
            .await?
            .ok_or_else(|| TransactionManagerError::NotFound {
                entity: "Transaction result",
                key: transaction_id.to_string(),
            })
    }

    pub async fn get_substate(
        &self,
        substate_requirement: &SubstateRequirement,
    ) -> Result<SubstateResult, TransactionManagerError> {
        let address = substate_requirement.to_substate_address_zero_version();
        let result = self
            .network_client
            .try_with_committee(iter::once(address), 1, |mut client| async move {
                client.get_substate(substate_requirement.as_ref()).await
            })
            .await?;
        Ok(result)
    }
}
