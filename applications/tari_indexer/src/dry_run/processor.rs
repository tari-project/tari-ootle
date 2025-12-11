//  Copyright 2023. The Tari Project
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
    collections::{HashMap, HashSet},
    sync::Arc,
};

use log::{debug, info};
use tari_engine::{fees::FeeTable, state_store::new_memory_store, traits::ClaimProofVerifier};
use tari_engine_types::{
    commit_result::ExecuteResult,
    published_template::{PublishedTemplate, PublishedTemplateAddress},
    substate::{Substate, SubstateId},
    virtual_substate::{VirtualSubstate, VirtualSubstateId, VirtualSubstates},
};
use tari_epoch_manager::{service::EpochManagerHandle, EpochManagerReader};
use tari_ootle_app_utilities::transaction_executor::{TariTransactionProcessor, TransactionExecutor as _};
use tari_ootle_common_types::{
    optional::Optional,
    services::template_provider::TemplateProvider,
    Epoch,
    PeerAddress,
    SubstateRequirement,
};
use tari_ootle_storage::global::TemplateStatus;
use tari_transaction::Transaction;
use tokio::task;

use crate::{
    dry_run::{error::DryRunTransactionProcessorError, package::Package},
    substate_manager::SubstateManager,
    template_manager::{TemplateCode, TemplateManager, TemplateManagerError},
};

const LOG_TARGET: &str = "tari::indexer::dry_run_transaction_processor";

#[derive(Clone)]
pub struct DryRunTransactionProcessor {
    fee_table: FeeTable,
    epoch_manager: EpochManagerHandle<PeerAddress>,
    template_manager: TemplateManager<PeerAddress>,
    substate_manager: SubstateManager,
    claim_burn_proof_verifier: Arc<dyn ClaimProofVerifier + Send + Sync + 'static>,
}

impl DryRunTransactionProcessor {
    pub fn new(
        fee_table: FeeTable,
        epoch_manager: EpochManagerHandle<PeerAddress>,
        template_manager: TemplateManager<PeerAddress>,
        substate_manager: SubstateManager,
        claim_burn_proof_verifier: impl ClaimProofVerifier + Send + Sync + 'static,
    ) -> Self {
        Self {
            fee_table,
            epoch_manager,
            template_manager,
            substate_manager,
            claim_burn_proof_verifier: Arc::new(claim_burn_proof_verifier),
        }
    }

    pub async fn process_transaction(
        &self,
        transaction: Transaction,
    ) -> Result<ExecuteResult, DryRunTransactionProcessorError> {
        if !transaction.is_dry_run() {
            return Err(DryRunTransactionProcessorError::NonDryRunTransaction);
        }

        info!(target: LOG_TARGET, "process_transaction: {}", transaction.calculate_id());

        let epoch = self.epoch_manager.current_epoch().await?;
        let found_substates = self.fetch_input_substates(&transaction).await?;
        let package = self.construct_template_package(&transaction, &found_substates).await?;

        let virtual_substates = self.get_virtual_substates(&transaction, epoch).await?;

        let mut state_store = new_memory_store();
        state_store.set_many(found_substates)?;

        // execute the payload in the WASM engine and return the result
        let processor =
            TariTransactionProcessor::new(package, self.fee_table.clone(), self.claim_burn_proof_verifier.clone());
        let exec_output = task::spawn_blocking(move || {
            processor.execute(&transaction, state_store.into_read_only(), virtual_substates)
        })
        .await??;

        Ok(exec_output.result)
    }

    async fn construct_template_package(
        &self,
        transaction: &Transaction,
        inputs: &HashMap<SubstateId, Substate>,
    ) -> Result<Package, DryRunTransactionProcessorError> {
        let component_templates = inputs.values().filter_map(|substate| {
            substate
                .substate_value()
                .as_component()
                .map(|component| &component.template_address)
        });

        let req_templates = transaction
            .referenced_templates_iter()
            .chain(component_templates)
            .collect::<HashSet<_>>();

        let mut package = Package::builder(req_templates.len());
        debug!(
            target: LOG_TARGET,
            "Fetching {} required templates for transaction {}",
            req_templates.len(),
            transaction.calculate_id()
        );

        for address in req_templates {
            match self.template_manager.get_template(address)? {
                Some(template) => {
                    debug!(
                        target: LOG_TARGET,
                        "Template {} found in local template manager cache",
                        address
                    );
                    package.add_template(*address, template);
                },
                None => {
                    debug!(
                        target: LOG_TARGET,
                        "Template {} not found in local cache. Fetching from validator nodes...",
                        address
                    );
                    // Fetch and cache
                    let substate_id = PublishedTemplateAddress::from_template_address(*address);
                    // TODO: batch missing and fetch templates in one/a few requests
                    let substate = self
                        .fetch_substate(&SubstateRequirement::unversioned(substate_id))
                        .await
                        .optional()?
                        .ok_or_else(|| {
                            DryRunTransactionProcessorError::TemplateManagerError(
                                TemplateManagerError::TemplateNotFound { address: *address },
                            )
                        })?;

                    let template: PublishedTemplate = substate.into_substate_value().into_template().ok_or(
                        DryRunTransactionProcessorError::InvariantViolation {
                            details: format!("Expected template substate at address {}", substate_id),
                        },
                    )?;

                    let loaded = self.template_manager.add_and_load_template(
                        template.author,
                        substate_id.as_template_address(),
                        TemplateCode::CompiledWasm(template.binary.into_bytes()),
                        TemplateStatus::Active,
                        Epoch(template.at_epoch),
                    )?;

                    package.add_template(substate_id.as_template_address(), loaded);
                },
            }
        }

        Ok(package.build())
    }

    async fn fetch_input_substates(
        &self,
        transaction: &Transaction,
    ) -> Result<HashMap<SubstateId, Substate>, DryRunTransactionProcessorError> {
        let mut substates = HashMap::with_capacity(transaction.inputs().len());

        // Fetch explicit inputs that may not have been resolved by the autofiller
        for requirement in transaction.inputs() {
            let substate = self.fetch_substate(requirement).await?;
            substates.insert(requirement.substate_id.clone(), substate);
        }

        Ok(substates)
    }

    async fn fetch_substate(
        &self,
        substate_requirement: &SubstateRequirement,
    ) -> Result<Substate, DryRunTransactionProcessorError> {
        let response = self
            .substate_manager
            .get_substate(substate_requirement.substate_id(), substate_requirement.version())
            .await?
            .ok_or_else(|| DryRunTransactionProcessorError::SubstateNotFound {
                requirement: substate_requirement.clone(),
            })?;

        Ok(Substate::new(response.version, response.substate))
    }

    async fn get_virtual_substates(
        &self,
        _transaction: &Transaction,
        epoch: Epoch,
    ) -> Result<VirtualSubstates, DryRunTransactionProcessorError> {
        let mut virtual_substates = VirtualSubstates::new();

        virtual_substates.insert(
            VirtualSubstateId::CurrentEpoch,
            VirtualSubstate::CurrentEpoch(epoch.as_u64()),
        );

        Ok(virtual_substates)
    }
}
