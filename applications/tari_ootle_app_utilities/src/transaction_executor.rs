//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use tari_engine::{
    executables::Executable,
    fees::{FeeModule, FeeTable},
    runtime::{AuthParams, RuntimeModule},
    state_store::{memory::ReadOnlyMemoryStateStore, StateStoreError},
    template::LoadedTemplate,
    traits::ClaimProofVerifier,
    transaction::{ModulesCollection, TransactionError, TransactionProcessor},
};
use tari_engine_types::{commit_result::ExecuteResult, substate::Substate, virtual_substate::VirtualSubstates};
use tari_ootle_common_types::{
    services::template_provider::TemplateProvider,
    SubstateLockType,
    SubstateRequirement,
    VersionedSubstateId,
};
use tari_ootle_storage::consensus_models::VersionedSubstateIdLockIntent;
use tari_template_lib::prelude::NonFungibleAddress;
use tari_transaction::Transaction;

const _LOG_TARGET: &str = "tari::ootle::transaction_executor";

pub trait TransactionExecutor {
    type Error: std::error::Error + Send + Sync + 'static;

    fn execute(
        &self,
        transaction: &Transaction,
        state_store: ReadOnlyMemoryStateStore,
        virtual_substates: VirtualSubstates,
    ) -> Result<ExecutionOutput, Self::Error>;
}

#[derive(Debug, Clone)]
pub struct ExecutionOutput {
    pub result: ExecuteResult,
}

impl ExecutionOutput {
    pub fn resolve_input_locks<'a, I: IntoIterator<Item = (&'a SubstateRequirement, &'a Substate)>>(
        &self,
        inputs: I,
    ) -> Vec<VersionedSubstateIdLockIntent> {
        if let Some(diff) = self.result.finalize.any_accept() {
            inputs
                .into_iter()
                .map(|(substate_req, substate)| {
                    let requested_specific_version = substate_req.version().is_some();
                    let lock_flag = if diff.down_iter().any(|(id, _)| id == substate_req.substate_id()) {
                        // Update all inputs that were DOWNed to be write locked
                        SubstateLockType::Write
                    } else {
                        // Any input not downed, gets a read lock
                        SubstateLockType::Read
                    };
                    VersionedSubstateIdLockIntent::new(
                        VersionedSubstateId::new(substate_req.substate_id().clone(), substate.version()),
                        lock_flag,
                        requested_specific_version,
                    )
                })
                .collect()
        } else {
            // TODO: we might want to have a SubstateLockFlag::None for rejected transactions so that we still know the
            // shards involved but do not lock them. We dont actually lock anything for rejected transactions anyway.
            inputs
                .into_iter()
                .map(|(substate_req, substate)| {
                    VersionedSubstateIdLockIntent::new(
                        VersionedSubstateId::new(substate_req.substate_id().clone(), substate.version()),
                        SubstateLockType::Read,
                        true,
                    )
                })
                .collect()
        }
    }
}

#[derive(Clone)]
pub struct TariTransactionProcessor<TTemplateProvider> {
    template_provider: Arc<TTemplateProvider>,
    modules: ModulesCollection,
    claim_burn_proof_verifier: Arc<dyn ClaimProofVerifier + Send + Sync + 'static>,
}

impl<TTemplateProvider> TariTransactionProcessor<TTemplateProvider> {
    pub fn new(
        template_provider: TTemplateProvider,
        fee_table: FeeTable,
        claim_burn_proof_verifier: Arc<dyn ClaimProofVerifier + Send + Sync + 'static>,
    ) -> Self {
        let modules = vec![Box::new(FeeModule::new(0, fee_table)) as Box<dyn RuntimeModule>];
        Self {
            template_provider: Arc::new(template_provider),
            modules: Arc::from(modules),
            claim_burn_proof_verifier,
        }
    }
}

impl<TTemplateProvider> TransactionExecutor for TariTransactionProcessor<TTemplateProvider>
where TTemplateProvider: TemplateProvider<Template = LoadedTemplate>
{
    type Error = TransactionProcessorError;

    fn execute(
        &self,
        transaction: &Transaction,
        state_store: ReadOnlyMemoryStateStore,
        virtual_substates: VirtualSubstates,
    ) -> Result<ExecutionOutput, Self::Error> {
        // Include signature public key badges for all transaction signers in the initial auth scope
        // NOTE: we assume all signatures have already been validated.
        let initial_ownership_proofs = transaction
            .signers_iter()
            .map(|pk| NonFungibleAddress::from_public_key(*pk))
            .collect();
        let auth_params = AuthParams {
            initial_ownership_proofs: Arc::new(initial_ownership_proofs),
        };

        let processor = TransactionProcessor::new(
            self.template_provider.clone(),
            state_store,
            auth_params,
            virtual_substates,
            self.modules.clone(),
            self.claim_burn_proof_verifier.clone(),
        );
        let result = processor.execute(transaction.clone())?;

        Ok(ExecutionOutput { result })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TransactionProcessorError {
    #[error(transparent)]
    TransactionError(#[from] TransactionError),
    #[error(transparent)]
    StateStoreError(#[from] StateStoreError),
}
