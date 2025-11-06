//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, convert::Infallible, future::Future, pin::Pin, time::Duration};

use futures::Stream;
use serde::{Deserialize, Serialize};
use tari_consensus_types::Decision;
use tari_engine_types::{
    commit_result::ExecuteResult,
    substate::{Substate, SubstateId, SubstateValue},
    Utxo,
};
use tari_ootle_common_types::{shard::Shard, Epoch, StateVersion};
use tari_template_abi::TemplateDef;
use tari_template_lib::{
    models::{ResourceAddress, UtxoId},
    prelude::{RistrettoPublicKeyBytes, TemplateAddress},
    types::crypto::UtxoTag,
};
use tari_transaction::{Transaction, TransactionId};
use time::PrimitiveDateTime;

use crate::models::UtxoUpdatePayload;

/// A pinned, boxed stream of UTXO updates
pub type UtxoUpdateStream<E> = Pin<Box<dyn Stream<Item = Result<UtxoUpdatePayload, E>> + Send + 'static>>;

pub trait WalletNetworkInterface {
    type Error: std::error::Error + Send + Sync + 'static;

    fn query_substate(
        &self,
        address: &SubstateId,
        version: Option<u32>,
        local_search_only: bool,
    ) -> impl Future<Output = Result<SubstateQueryResult, Self::Error>> + Send;

    fn get_substates(
        &self,
        substate_ids: Vec<SubstateId>,
    ) -> impl Future<Output = Result<HashMap<SubstateId, Substate>, Self::Error>> + Send;

    fn submit_transaction(
        &self,
        transaction: Transaction,
    ) -> impl Future<Output = Result<TransactionId, Self::Error>> + Send;

    fn submit_dry_run_transaction(
        &self,
        transaction: Transaction,
    ) -> impl Future<Output = Result<TransactionQueryResult, Self::Error>> + Send;

    fn query_transaction_result(
        &self,
        transaction_id: TransactionId,
    ) -> impl Future<Output = Result<TransactionQueryResult, Self::Error>> + Send;

    fn fetch_template_definition(
        &self,
        template_address: TemplateAddress,
    ) -> impl Future<Output = Result<TemplateDef, Self::Error>> + Send;

    fn stream_stealth_utxo_updates(
        &self,
        from_epoch: Epoch,
        resource_address: ResourceAddress,
        shard_state_versions: Vec<(Shard, StateVersion)>,
        unspent_only: bool,
    ) -> impl Future<Output = Result<UtxoUpdateStream<Self::Error>, Self::Error>> + Send;

    fn get_unspent_utxos(
        &self,
        resource_address: ResourceAddress,
        tag_and_nonce_pairs: Vec<(UtxoTag, RistrettoPublicKeyBytes)>,
    ) -> impl Future<Output = Result<Vec<(UtxoId, Utxo)>, Self::Error>> + Send;

    fn wait_until_ready(&self) -> impl Future<Output = Result<(), Self::Error>> + Send;
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
    pub version: u32,
    pub substate: SubstateValue,
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
