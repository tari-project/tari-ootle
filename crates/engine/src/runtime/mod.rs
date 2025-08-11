//  Copyright 2022. The Tari Project
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

mod auth;
pub use auth::{AuthParams, AuthorizationScope};

mod r#impl;
pub use r#impl::RuntimeInterfaceImpl;

mod engine_args;
pub use crate::runtime::engine_args::EngineArgs;

mod error;
pub use error::{AssertError, RuntimeError, TransactionCommitError};

mod actions;
pub use actions::*;

mod module;
pub use module::{RuntimeModule, RuntimeModuleError};

mod fee_state;
mod tracker;

mod locking;
pub mod scope;
pub use locking::{LockError, LockState};
mod address_allocation;
mod state_store;
mod tracker_auth;
mod validation;
mod working_state;
mod workspace;

use std::{fmt::Debug, sync::Arc};

use tari_bor::decode_exact;
use tari_engine_types::{
    commit_result::FinalizeResult,
    component::ComponentHeader,
    confidential::ConfidentialClaim,
    indexed_value::IndexedValue,
    limits,
    lock::LockFlag,
    substate::SubstateValue,
    ComponentCall,
    ResourceAddressRef,
    ValidatorFeePoolAddress,
};
use tari_template_lib::{
    args::{
        AddressAllocationInvokeArg,
        AllocatableAddressType,
        AllocateAddressResult,
        BucketAction,
        BucketRef,
        BuiltinTemplateAction,
        CallAction,
        CallerContextAction,
        ComponentAction,
        ComponentRef,
        ConsensusAction,
        GenerateRandomAction,
        InstructionArg,
        InvokeResult,
        LogLevel,
        NonFungibleAction,
        ProofAction,
        ProofRef,
        ResourceAction,
        ResourceRef,
        VaultAction,
        WorkspaceAction,
        WorkspaceId,
        WorkspaceOffsetId,
    },
    invoke_args,
    models::{BucketId, ComponentAddress, Metadata, NonFungibleAddress, StealthTransferStatement, VaultRef},
    types::EntityId,
};
pub use tracker::StateTracker;

use crate::runtime::{error::ArgumentValidationError, locking::LockedSubstate, scope::PushCallFrame};

pub trait RuntimeInterface: Send + Sync {
    fn next_entity_id(&self) -> Result<EntityId, RuntimeError>;
    fn emit_event(&self, topic: String, payload: Metadata) -> Result<(), RuntimeError>;

    fn emit_log(&self, level: LogLevel, message: String) -> Result<(), RuntimeError>;

    fn load_component(&self, call: ComponentCall) -> Result<(ComponentAddress, ComponentHeader), RuntimeError>;

    fn lock_component(&self, address: ComponentAddress, lock_flag: LockFlag) -> Result<LockedSubstate, RuntimeError>;

    fn get_substate(&self, lock: &LockedSubstate) -> Result<SubstateValue, RuntimeError>;
    fn component_invoke(
        &self,
        component_ref: ComponentRef,
        action: ComponentAction,
        args: EngineArgs,
    ) -> Result<InvokeResult, RuntimeError>;

    fn resource_invoke(
        &self,
        resource_ref: ResourceRef,
        action: ResourceAction,
        args: EngineArgs,
    ) -> Result<InvokeResult, RuntimeError>;

    fn vault_invoke(
        &self,
        vault_ref: VaultRef,
        action: VaultAction,
        args: EngineArgs,
    ) -> Result<InvokeResult, RuntimeError>;

    fn bucket_invoke(
        &self,
        bucket_ref: BucketRef,
        action: BucketAction,
        args: EngineArgs,
    ) -> Result<InvokeResult, RuntimeError>;

    fn proof_invoke(
        &self,
        proof_ref: ProofRef,
        action: ProofAction,
        args: EngineArgs,
    ) -> Result<InvokeResult, RuntimeError>;
    fn workspace_invoke(&self, action: WorkspaceAction, args: EngineArgs) -> Result<InvokeResult, RuntimeError>;

    fn non_fungible_invoke(
        &self,
        nf_addr: NonFungibleAddress,
        action: NonFungibleAction,
        args: EngineArgs,
    ) -> Result<InvokeResult, RuntimeError>;

    fn consensus_invoke(&self, action: ConsensusAction) -> Result<InvokeResult, RuntimeError>;

    fn generate_random_invoke(&self, action: GenerateRandomAction) -> Result<InvokeResult, RuntimeError>;

    fn generate_uuid(&self) -> Result<[u8; 32], RuntimeError>;

    fn set_last_instruction_output(&self, value: IndexedValue) -> Result<(), RuntimeError>;

    fn claim_burn(&self, claim: ConfidentialClaim) -> Result<(), RuntimeError>;

    fn claim_validator_fees(&self, address: ValidatorFeePoolAddress) -> Result<(), RuntimeError>;

    fn set_fee_checkpoint(&self) -> Result<(), RuntimeError>;
    fn reset_to_fee_checkpoint(&self) -> Result<(), RuntimeError>;
    fn finalize(&self) -> Result<FinalizeResult, RuntimeError>;
    fn validate_finalized(&self) -> Result<(), RuntimeError>;

    fn caller_context_invoke(
        &self,
        action: CallerContextAction,
        args: EngineArgs,
    ) -> Result<InvokeResult, RuntimeError>;

    fn allocate_address_invoke(&self, action: AddressAllocationInvokeArg) -> Result<InvokeResult, RuntimeError>;

    fn call_invoke(&self, action: CallAction, args: EngineArgs) -> Result<InvokeResult, RuntimeError>;

    fn builtin_template_invoke(&self, action: BuiltinTemplateAction) -> Result<InvokeResult, RuntimeError>;

    fn check_component_access_rules(&self, method: &str, locked: &LockedSubstate) -> Result<(), RuntimeError>;

    fn validate_return_value(&self, value: &IndexedValue) -> Result<(), RuntimeError>;

    fn push_call_frame(&self, frame: PushCallFrame) -> Result<(), RuntimeError>;
    fn pop_call_frame(&self) -> Result<(), RuntimeError>;
    fn publish_template(&self, template: Vec<u8>) -> Result<(), RuntimeError>;

    fn allocate_address(
        &self,
        substate_type: AllocatableAddressType,
        entity_id: EntityId,
        workspace_id: WorkspaceId,
    ) -> Result<AllocateAddressResult, RuntimeError>;

    fn stealth_transfer(
        &self,
        resource_address: ResourceAddressRef,
        statement: StealthTransferStatement,
        revealed_funds_bucket: Option<BucketId>,
    ) -> Result<Option<BucketId>, RuntimeError>;
}

#[derive(Clone)]
pub struct Runtime {
    interface: Arc<dyn RuntimeInterface>,
}

impl Runtime {
    pub(crate) fn resolve_args(&self, args: &[InstructionArg]) -> Result<Vec<tari_bor::Value>, RuntimeError> {
        if args.len() > limits::WASM_LIMITS.max_function_arguments {
            return Err(ArgumentValidationError::TooManyArguments {
                got: args.len(),
                max: limits::WASM_LIMITS.max_function_arguments,
            }
            .into());
        }
        let mut resolved = Vec::with_capacity(args.len());
        for arg in args {
            match arg {
                InstructionArg::Workspace(key) => {
                    let result = self.resolve_workspace_id(key)?;
                    resolved.push(result.into_value()?);
                },
                InstructionArg::Literal(v) => resolved.push(decode_exact(v)?),
            }
        }
        Ok(resolved)
    }

    pub(crate) fn resolve_workspace_id(&self, workspace_id: &WorkspaceOffsetId) -> Result<InvokeResult, RuntimeError> {
        self.interface
            .workspace_invoke(WorkspaceAction::Get, invoke_args![workspace_id].into())
    }
}

impl Runtime {
    pub fn new(interface: Arc<dyn RuntimeInterface>) -> Self {
        Self { interface }
    }

    pub fn interface(&self) -> &dyn RuntimeInterface {
        &*self.interface
    }
}

impl Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("interface", &"dyn RuntimeEngine")
            .finish()
    }
}
