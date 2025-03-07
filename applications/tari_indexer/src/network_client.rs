//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, fmt::Display, future::Future};

use log::{info, warn};
use tari_dan_common_types::{
    optional::IsNotFoundError,
    NodeAddressable,
    ShardGroup,
    SubstateAddress,
    ToSubstateAddress,
};
use tari_epoch_manager::{EpochManagerError, EpochManagerReader};
use tari_transaction::{Transaction, TransactionId};
use tari_validator_node_rpc::client::{ValidatorNodeClientFactory, ValidatorNodeRpcClient};

const LOG_TARGET: &str = "tari::indexer::network_client";

#[derive(Debug, Clone)]
pub struct TariNetworkClient<TEpochManager, TClientFactory> {
    epoch_manager: TEpochManager,
    client_provider: TClientFactory,
}

impl<TAddr, TEpochManager, TClientFactory> TariNetworkClient<TEpochManager, TClientFactory>
where
    TAddr: NodeAddressable + 'static,
    TEpochManager: EpochManagerReader<Addr = TAddr> + 'static,
    TClientFactory: ValidatorNodeClientFactory<TAddr> + 'static,
    <TClientFactory::Client as ValidatorNodeRpcClient<TAddr>>::Error: IsNotFoundError + 'static,
{
    pub fn new(epoch_manager: TEpochManager, client_provider: TClientFactory) -> Self {
        Self {
            epoch_manager,
            client_provider,
        }
    }

    pub async fn submit_transaction(&self, transaction: Transaction) -> Result<TransactionId, NetworkClientError> {
        if transaction.num_unique_inputs() == 0 {
            return Err(NetworkClientError::NoInputsProvided);
        }

        let tx_id = *transaction.id();

        info!(
            target: LOG_TARGET,
            "Submitting transaction {} to the validator node", tx_id
        );

        // Ensure initial scanning has completed to ensure an accurate epoch
        self.epoch_manager.wait_for_initial_scanning_to_complete().await?;

        let involved = transaction
            .all_inputs_iter()
            // The version does not affect the shard group
            .map(|i| i.or_zero_version().to_substate_address())
            // NOTE: if I don't collect here, we get lifetime issues in the JSON-RPC handlers (Send impl not general enough).
            // For uniqueness, it seems like a good idea to collect to a HashSet anyway.
            .collect::<HashSet<_>>();
        self.try_with_committee(involved, 2, |mut client| {
            let transaction = transaction.clone();
            async move { client.submit_transaction(transaction).await }
        })
        .await
    }

    #[allow(dead_code)]
    pub async fn try_with_random_members<'a, F, T, E, TFut>(
        &self,
        num_to_query: usize,
        shard_group: Option<ShardGroup>,
        mut callback: F,
    ) -> Result<T, NetworkClientError>
    where
        F: FnMut(TClientFactory::Client) -> TFut,
        TFut: Future<Output = Result<T, E>> + 'a,
        TClientFactory::Client: 'a,
        T: 'static,
        E: Display,
    {
        let epoch = self.epoch_manager.current_epoch().await?;
        let mut attempted = vec![];

        let mut last_error = None;
        while attempted.len() < num_to_query {
            let vn = self
                .epoch_manager
                .get_random_committee_member(epoch, shard_group, attempted.clone())
                .await?;

            let client = self.client_provider.create_client(&vn.address);
            match callback(client).await {
                Ok(ret) => {
                    return Ok(ret);
                },
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        "Request failed for validator '{}': {}", vn, err
                    );
                    last_error = Some(err.to_string());
                },
            }

            attempted.push(vn.address);
        }

        Err(NetworkClientError::AllValidatorsFailed {
            committee_size: attempted.len(),
            last_error,
        })
    }

    /// Fetches the committee members for the given shard and calls the given callback with each member until
    /// the callback returns a `Ok` result. If the callback returns an `Err` result, the next committee member is
    /// called.
    pub async fn try_with_committee<'a, F, T, E, TFut, ISubstateAddr>(
        &self,
        substate_addresses: ISubstateAddr,
        mut num_to_query: usize,
        mut callback: F,
    ) -> Result<T, NetworkClientError>
    where
        F: FnMut(TClientFactory::Client) -> TFut,
        TFut: Future<Output = Result<T, E>> + 'a,
        TClientFactory::Client: 'a,
        T: 'static,
        E: Display,
        ISubstateAddr: IntoIterator<Item = SubstateAddress>,
    {
        let epoch = self.epoch_manager.current_epoch().await?;
        // Get all unique members. The hashset already "shuffles" items owing to the random hash function.
        let mut all_members = HashSet::new();
        // TODO: suggest passing in the shard groups to try_with_committee. We need the NumPreshards and
        // num_committees from the epoch manager to do so but this will also prevent us loading the same committees
        // multiple times.
        for substate_address in substate_addresses {
            let committee = self
                .epoch_manager
                .get_committee_for_substate(epoch, substate_address)
                .await?;
            all_members.extend(committee.into_addresses());
        }

        let committee_size = all_members.len();
        if committee_size == 0 {
            return Err(NetworkClientError::NoCommitteeMembers);
        }

        let mut num_succeeded = 0;
        let mut last_error = None;
        let mut last_return = None;
        for validator in all_members {
            let client = self.client_provider.create_client(&validator);
            match callback(client).await {
                Ok(ret) => {
                    num_to_query = num_to_query.saturating_sub(1);
                    num_succeeded += 1;
                    last_return = Some(ret);
                    if num_to_query == 0 {
                        break;
                    }
                },
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        "Request failed for validator '{}': {}", validator, err
                    );
                    last_error = Some(err.to_string());
                },
            }
        }

        if num_succeeded == 0 {
            return Err(NetworkClientError::AllValidatorsFailed {
                committee_size,
                last_error,
            });
        }

        Ok(last_return.expect("last_return must be Some if num_succeeded > 0"))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NetworkClientError {
    #[error("Epoch manager error: {0}")]
    EpochManagerError(#[from] EpochManagerError),
    #[error("Rpc call failed for all ({committee_size}) validators: {}", .last_error.as_deref().unwrap_or("unknown"))]
    AllValidatorsFailed {
        committee_size: usize,
        last_error: Option<String>,
    },
    #[error("No committee at present. Try again later")]
    NoCommitteeMembers,
    #[error("No inputs provided in transaction.")]
    NoInputsProvided,
}
