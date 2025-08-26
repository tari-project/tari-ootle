//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::{Arc, Mutex};

use axum::async_trait;
use reqwest::{IntoUrl, Url};
use tari_engine_types::substate::SubstateId;
use tari_indexer_client::{
    error::IndexerClientError,
    json_rpc_client::IndexerJsonRpcClient,
    types::{
        GetSubstateRequest,
        GetTransactionResultRequest,
        IndexerTransactionFinalizedResult,
        ListSubstatesRequest,
        SubmitTransactionRequest,
    },
};
use tari_ootle_common_types::{optional::IsNotFoundError, substate_type::SubstateType};
use tari_ootle_wallet_sdk::network::{
    StatusResponseError,
    SubstateListItem,
    SubstateListResult,
    SubstateQueryResult,
    TransactionFinalizedResult,
    TransactionQueryResult,
    WalletNetworkInterface,
    WalletQueryErrorStatus,
};
use tari_template_lib::types::TemplateAddress;
use tari_transaction::{Transaction, TransactionId};
use url::ParseError;

use crate::jrpc_server::ApplicationErrorCode;

#[derive(Debug, Clone)]
pub struct IndexerJsonRpcNetworkInterface {
    indexer_jrpc_address: Arc<Mutex<Url>>,
}

impl IndexerJsonRpcNetworkInterface {
    pub fn new<T: IntoUrl>(indexer_jrpc_address: T) -> Self {
        Self {
            indexer_jrpc_address: Arc::new(Mutex::new(
                indexer_jrpc_address
                    .into_url()
                    .expect("Malformed indexer JSON-RPC address"),
            )),
        }
    }

    fn get_client(&self) -> Result<IndexerJsonRpcClient, IndexerJrpcError> {
        let client = IndexerJsonRpcClient::connect((*self.indexer_jrpc_address.lock().unwrap()).clone())?;
        Ok(client)
    }

    pub fn set_endpoint(&mut self, endpoint: &str) -> Result<(), IndexerJrpcError> {
        *self.indexer_jrpc_address.lock().unwrap() = Url::parse(endpoint)?;
        Ok(())
    }

    pub fn get_endpoint(&self) -> Url {
        (*self.indexer_jrpc_address.lock().unwrap()).clone()
    }
}

#[async_trait]
impl WalletNetworkInterface for IndexerJsonRpcNetworkInterface {
    type Error = IndexerJrpcError;

    async fn query_substate(
        &self,
        substate_id: &SubstateId,
        version: Option<u32>,
        local_search_only: bool,
    ) -> Result<SubstateQueryResult, Self::Error> {
        let mut client = self.get_client()?;
        let result = client
            .get_substate(GetSubstateRequest {
                address: substate_id.clone(),
                version,
                local_search_only,
            })
            .await?;
        Ok(SubstateQueryResult {
            address: result.address,
            version: result.version,
            substate: result.substate,
        })
    }

    async fn list_substates(
        &self,
        filter_by_template: Option<TemplateAddress>,
        filter_by_type: Option<SubstateType>,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<SubstateListResult, Self::Error> {
        let mut client = self.get_client()?;
        let result = client
            .list_substates(ListSubstatesRequest {
                filter_by_template,
                filter_by_type,
                limit,
                offset,
            })
            .await?;
        let substates = result
            .substates
            .into_iter()
            .map(|s| SubstateListItem {
                substate_id: s.substate_id,
                module_name: s.module_name,
                version: s.version,
                template_address: s.template_address,
                timestamp: s.timestamp,
            })
            .collect();
        Ok(SubstateListResult { substates })
    }

    async fn submit_transaction(&self, transaction: Transaction) -> Result<TransactionId, Self::Error> {
        let mut client = self.get_client()?;
        let result = client
            .submit_transaction(SubmitTransactionRequest {
                transaction,
                is_dry_run: false,
            })
            .await?;
        Ok(result.transaction_id)
    }

    async fn submit_dry_run_transaction(
        &self,
        transaction: Transaction,
    ) -> Result<TransactionQueryResult, Self::Error> {
        let mut client = self.get_client()?;
        let resp = client
            .submit_transaction(SubmitTransactionRequest {
                transaction,
                is_dry_run: true,
            })
            .await?;

        Ok(TransactionQueryResult {
            transaction_id: resp.transaction_id,
            result: convert_indexer_result_to_wallet_result(resp.result),
        })
    }

    async fn query_transaction_result(
        &self,
        transaction_id: TransactionId,
    ) -> Result<TransactionQueryResult, Self::Error> {
        let mut client = self.get_client()?;
        let resp = client
            .get_transaction_result(GetTransactionResultRequest { transaction_id })
            .await?;

        Ok(TransactionQueryResult {
            transaction_id,
            result: convert_indexer_result_to_wallet_result(resp.result),
        })
    }

    async fn fetch_template_definition(
        &self,
        template_address: TemplateAddress,
    ) -> Result<tari_template_abi::TemplateDef, Self::Error> {
        let mut client = self.get_client()?;
        let resp = client
            .get_template_definition(tari_indexer_client::types::GetTemplateDefinitionRequest { template_address })
            .await?;

        Ok(resp.definition)
    }

    async fn wait_until_ready(&self) -> Result<(), Self::Error> {
        let mut client = self.get_client()?;
        client.wait_until_ready().await?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IndexerJrpcError {
    #[error("Indexer client error: {0}")]
    IndexerClientError(#[from] IndexerClientError),
    #[error("Indexer parse error : {0}")]
    IndexerParseError(#[from] ParseError),
}

impl IsNotFoundError for IndexerJrpcError {
    fn is_not_found_error(&self) -> bool {
        match self {
            IndexerJrpcError::IndexerClientError(err) => err.is_not_found_error(),
            _ => false,
        }
    }
}

impl StatusResponseError for IndexerJrpcError {
    fn get_status(&self) -> WalletQueryErrorStatus {
        match self {
            IndexerJrpcError::IndexerClientError(err) => {
                if err.is_not_found_error() {
                    return WalletQueryErrorStatus::NotFound {
                        message: "The requested resource was not found".to_string(),
                    };
                }
                match err {
                    IndexerClientError::RequestFailedWithStatus { code, message }
                        if *code == ApplicationErrorCode::InvalidRequest as i64 =>
                    {
                        WalletQueryErrorStatus::TransactionRejected {
                            message: message.clone(),
                        }
                    },
                    IndexerClientError::RequestFailedWithStatus { code, message } => {
                        WalletQueryErrorStatus::InternalError {
                            message: format!("Indexer request failed with status {code}: {message}"),
                        }
                    },
                    _ => WalletQueryErrorStatus::InternalError {
                        message: format!("Indexer client error: {err}"),
                    },
                }
            },
            IndexerJrpcError::IndexerParseError(e) => WalletQueryErrorStatus::InternalError {
                message: format!("Indexer parse error: {e}"),
            },
        }
    }

    fn get_error_message(&self) -> String {
        self.to_string()
    }
}

/// These types are identical, however in order to keep the wallet decoupled from the indexer, we define two types and
/// this conversion function.
// TODO: the common interface and types between the wallet and indexer could be made into a shared "view of the network"
// interface and we can avoid defining two types.
fn convert_indexer_result_to_wallet_result(result: IndexerTransactionFinalizedResult) -> TransactionFinalizedResult {
    match result {
        IndexerTransactionFinalizedResult::Pending => TransactionFinalizedResult::Pending,
        IndexerTransactionFinalizedResult::Finalized {
            final_decision,
            execution_result,
            finalized_time,
            execution_time,
            abort_details,
        } => TransactionFinalizedResult::Finalized {
            final_decision,
            execution_result,
            execution_time,
            finalized_time,
            abort_details,
        },
    }
}
