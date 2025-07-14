//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{convert::Infallible, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tari_consensus_types::Decision;
use tari_engine_types::{
    commit_result::ExecuteResult,
    substate::{SubstateId, SubstateValue},
};
use tari_ootle_common_types::substate_type::SubstateType;
use tari_template_abi::TemplateDef;
use tari_template_lib::prelude::TemplateAddress;
use tari_transaction::{Transaction, TransactionId};
use time::PrimitiveDateTime;

#[async_trait]
pub trait WalletNetworkInterface {
    type Error: std::error::Error + Send + Sync + 'static;

    async fn query_substate(
        &self,
        address: &SubstateId,
        version: Option<u32>,
        local_search_only: bool,
    ) -> Result<SubstateQueryResult, Self::Error>;

    async fn list_substates(
        &self,
        filter_by_template: Option<TemplateAddress>,
        filter_by_type: Option<SubstateType>,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<SubstateListResult, Self::Error>;

    async fn submit_transaction(&self, transaction: Transaction) -> Result<TransactionId, Self::Error>;

    async fn submit_dry_run_transaction(&self, transaction: Transaction)
        -> Result<TransactionQueryResult, Self::Error>;

    async fn query_transaction_result(
        &self,
        transaction_id: TransactionId,
    ) -> Result<TransactionQueryResult, Self::Error>;

    async fn fetch_template_definition(&self, template_address: TemplateAddress) -> Result<TemplateDef, Self::Error>;

    async fn wait_until_ready(&self) -> Result<(), Self::Error>;
}

/// A trait for responses that can provide a [WalletQueryErrorStatus]
pub trait StatusResponseError {
    fn get_status(&self) -> WalletQueryErrorStatus;
    fn get_error_message(&self) -> String;
}

// This is required for tests (PanicInterface) - in general, if a type is `Infallible` it should never reach the error.
impl StatusResponseError for Infallible {
    fn get_status(&self) -> WalletQueryErrorStatus {
        unreachable!("Infallible should never be used as an error type in this context")
    }

    fn get_error_message(&self) -> String {
        unreachable!("Infallible should never be used as an error type in this context")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WalletQueryErrorStatus {
    #[error("Not found: {message}")]
    NotFound { message: String },
    #[error("Transaction rejected: {message}")]
    TransactionRejected { message: String },
    #[error("Internal error: {message}")]
    InternalError { message: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubstateQueryResult {
    pub address: SubstateId,
    pub version: u32,
    pub substate: SubstateValue,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubstateListResult {
    pub substates: Vec<SubstateListItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubstateListItem {
    pub substate_id: SubstateId,
    pub module_name: Option<String>,
    pub version: u32,
    pub template_address: Option<TemplateAddress>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TransactionQueryResult {
    pub result: TransactionFinalizedResult,
    pub transaction_id: TransactionId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionFinalizedResult {
    Pending,
    Finalized {
        final_decision: Decision,
        execution_result: Option<Box<ExecuteResult>>,
        execution_time: Duration,
        finalized_time: PrimitiveDateTime,
        abort_details: Option<String>,
    },
}

impl TransactionFinalizedResult {
    pub fn into_execute_result(self) -> Option<ExecuteResult> {
        match self {
            TransactionFinalizedResult::Pending => None,
            TransactionFinalizedResult::Finalized { execution_result, .. } => execution_result.map(|r| *r),
        }
    }
}
