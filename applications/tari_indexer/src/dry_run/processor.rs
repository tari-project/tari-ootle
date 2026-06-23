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

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use log::info;
use tari_engine::{fees::FeeTable, state_store::new_memory_store, traits::ClaimProofVerifier};
use tari_engine_types::{
    commit_result::ExecuteResult,
    substate::{Substate, SubstateId},
    virtual_substate::{VirtualSubstate, VirtualSubstateId, VirtualSubstates},
};
use tari_epoch_manager::{EpochManagerReader, service::EpochManagerHandle};
use tari_ootle_app_utilities::transaction_executor::{TariTransactionProcessor, TransactionExecutor as _};
use tari_ootle_common_types::SubstateRequirementRef;
use tari_ootle_p2p::PeerAddress;
use tari_ootle_transaction::Transaction;
use tari_template_lib_types::constants::TARI_TOKEN;
use tokio::{runtime::Handle, task};

use crate::{
    dry_run::{
        error::DryRunTransactionProcessorError,
        template_provider::{DryRunTemplateProvider, build_dry_run_template_provider},
    },
    substate_manager::SubstateManager,
};

const LOG_TARGET: &str = "tari::indexer::dry_run_transaction_processor";

#[derive(Clone)]
pub struct DryRunTransactionProcessor {
    fee_table: FeeTable,
    epoch_manager: EpochManagerHandle<PeerAddress>,
    template_provider: DryRunTemplateProvider,
    substate_manager: SubstateManager,
    claim_burn_proof_verifier: Arc<dyn ClaimProofVerifier + Send + Sync + 'static>,
}

impl DryRunTransactionProcessor {
    pub fn new(
        fee_table: FeeTable,
        epoch_manager: EpochManagerHandle<PeerAddress>,
        substate_manager: SubstateManager,
        wasm_cache_dir: PathBuf,
        claim_burn_proof_verifier: impl ClaimProofVerifier + Send + Sync + 'static,
    ) -> Result<Self, std::io::Error> {
        let handle = Handle::try_current().map_err(std::io::Error::other)?;
        let template_provider = build_dry_run_template_provider(handle, substate_manager.clone(), wasm_cache_dir)?;
        Ok(Self {
            fee_table,
            epoch_manager,
            template_provider,
            substate_manager,
            claim_burn_proof_verifier: Arc::new(claim_burn_proof_verifier),
        })
    }

    pub async fn process_transaction(
        &self,
        transaction: Transaction,
    ) -> Result<ExecuteResult, DryRunTransactionProcessorError> {
        if !transaction.is_dry_run() {
            return Err(DryRunTransactionProcessorError::NonDryRunTransaction);
        }

        info!(target: LOG_TARGET, "process_transaction: {}", transaction.calculate_id());

        let mut found_substates = self.fetch_input_substates(&transaction).await?;
        // Add the TARI resource - this is what consensus does, so we'll need to do it for dry runs
        let id = TARI_TOKEN.into();
        if !found_substates.contains_key(&id) {
            let tari_token = self
                .substate_manager
                .get_substate(SubstateRequirementRef::unversioned(&id))
                .await?;
            found_substates.insert(TARI_TOKEN.into(), tari_token);
        }

        let virtual_substates = self.get_virtual_substates().await?;

        let mut state_store = new_memory_store();
        state_store.set_many(found_substates)?;

        // execute the payload in the WASM engine and return the result
        let processor = TariTransactionProcessor::new(
            self.template_provider.clone(),
            self.fee_table.clone(),
            true,
            self.claim_burn_proof_verifier.clone(),
        );
        let exec_output = task::spawn_blocking(move || {
            processor.execute(&transaction, state_store.into_read_only(), virtual_substates)
        })
        .await??;

        Ok(exec_output.result)
    }

    async fn fetch_input_substates(
        &self,
        transaction: &Transaction,
    ) -> Result<HashMap<SubstateId, Substate>, DryRunTransactionProcessorError> {
        let substates = self
            .substate_manager
            .get_substates(transaction.inputs().iter().map(|req| req.as_ref()))
            .await?;
        Ok(substates
            .into_iter()
            .map(|(id, fetched)| (id, fetched.substate))
            .collect())
    }

    async fn get_virtual_substates(&self) -> Result<VirtualSubstates, DryRunTransactionProcessorError> {
        let epoch = self.epoch_manager.current_epoch().await?;
        let epoch_hash = self
            .epoch_manager
            .get_current_epoch_hash()
            .await
            .map_err(DryRunTransactionProcessorError::EpochManager)?;
        Ok(VirtualSubstates::from_iter([
            (
                VirtualSubstateId::CurrentEpoch,
                VirtualSubstate::CurrentEpoch(epoch.as_u64()),
            ),
            (
                VirtualSubstateId::CurrentEpochHash,
                VirtualSubstate::CurrentEpochHash(epoch_hash.into_array().into()),
            ),
        ]))
    }
}
