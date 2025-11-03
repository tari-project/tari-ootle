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

use std::{sync::Arc, time::Instant};

use log::*;
use tari_bor::to_value;
use tari_engine_types::{
    commit_result::{ExecuteResult, FinalizeResult, RejectReason, TransactionResult},
    component::derive_component_address_from_public_key,
    entity_id_provider::EntityIdProvider,
    indexed_value::{IndexedValue, IndexedWellKnownTypes},
    instruction_result::InstructionResult,
    lock::LockFlag,
    virtual_substate::VirtualSubstates,
};
use tari_ootle_common_types::services::template_provider::TemplateProvider;
use tari_template_abi::{FunctionDef, Type};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::{
    args::{AllocateAddressResult, BucketAction, BucketRef, WorkspaceAction},
    auth::{ComponentAccessRules, OwnerRule},
    invoke_args,
    models::{Bucket, NonFungibleAddress, StealthTransferStatement},
    prelude::STEALTH_TARI_RESOURCE_ADDRESS,
    types::{crypto::RistrettoPublicKeyBytes, Amount, TemplateAddress},
};
use tari_transaction::{
    args::{InstructionArg, WorkspaceId, WorkspaceOffsetId},
    call_arg,
    call_args,
    AllocatableAddressType,
    ComponentCall,
    Instruction,
    ResourceAddressRef,
};

use crate::{
    executables::{Executable, WeightedExecutable},
    runtime::{
        scope::{CallScope, PushCallFrame},
        AuthParams,
        AuthorizationScope,
        Runtime,
        RuntimeError,
        RuntimeInterfaceImpl,
        RuntimeModule,
        StateTracker,
    },
    state_store::memory::ReadOnlyMemoryStateStore,
    template::LoadedTemplate,
    traits::{ClaimProofVerifier, Invokable},
    transaction::{TransactionError, TransactionProcessorConfig},
    wasm::{WasmModule, WasmProcess},
};

const LOG_TARGET: &str = "tari::ootle::engine::instruction_processor";
pub const MAX_CALL_DEPTH: usize = 10;
const ACCOUNT_CONSTRUCTOR_FUNCTION: &str = "create";

pub struct TransactionProcessor<TTemplateProvider> {
    config: TransactionProcessorConfig,
    template_provider: Arc<TTemplateProvider>,
    state_db: ReadOnlyMemoryStateStore,
    auth_params: AuthParams,
    virtual_substates: VirtualSubstates,
    modules: Vec<Arc<dyn RuntimeModule>>,
    claim_burn_proof_verifier: Arc<dyn ClaimProofVerifier + Send + Sync + 'static>,
}

impl<TTemplateProvider: TemplateProvider<Template = LoadedTemplate> + 'static> TransactionProcessor<TTemplateProvider> {
    pub fn new(
        config: TransactionProcessorConfig,
        template_provider: Arc<TTemplateProvider>,
        state_db: ReadOnlyMemoryStateStore,
        auth_params: AuthParams,
        virtual_substates: VirtualSubstates,
        modules: Vec<Arc<dyn RuntimeModule>>,
        claim_burn_proof_verifier: Arc<dyn ClaimProofVerifier + Send + Sync + 'static>,
    ) -> Self {
        Self {
            config,
            template_provider,
            state_db,
            auth_params,
            virtual_substates,
            modules,
            claim_burn_proof_verifier,
        }
    }

    #[allow(clippy::too_many_lines)]
    pub fn execute<E: Executable + WeightedExecutable>(self, executable: E) -> Result<ExecuteResult, TransactionError> {
        let id = executable.to_id();
        let timer = Instant::now();
        let entity_id_provider = EntityIdProvider::new(id.as_hash(), 1000);
        let Self {
            config,
            template_provider,
            state_db,
            auth_params,
            virtual_substates,
            modules,
            claim_burn_proof_verifier,
        } = self;

        let initial_auth_scope = AuthorizationScope::new(auth_params.initial_ownership_proofs);
        let mut initial_call_scope = CallScope::new();
        initial_call_scope.set_auth_scope(initial_auth_scope);
        // Because XTR resource is immutable, we can make it available to every shard group (genesis state) and
        // transaction (payment of fees)
        initial_call_scope.add_substate_to_owned(STEALTH_TARI_RESOURCE_ADDRESS.into());
        for input in executable.all_inputs_iter() {
            debug!(
                target: LOG_TARGET,
                "Adding substate to initial call scope: {}",
                input.substate_id
            );
            initial_call_scope.add_substate_to_owned(input.substate_id.clone());
        }

        let transaction_weight = executable.calculate_weight();
        let tracker = StateTracker::new(
            state_db,
            virtual_substates,
            initial_call_scope,
            id.as_hash(),
            transaction_weight,
        );

        let transaction_signer_public_key =
            executable
                .main_signer()
                .ok_or_else(|| TransactionError::InvariantError {
                    details: "Transaction must have at least one authorized signature".to_string(),
                })?;

        let runtime_interface = RuntimeInterfaceImpl::initialize(
            tracker,
            template_provider.clone(),
            transaction_signer_public_key,
            entity_id_provider,
            modules,
            MAX_CALL_DEPTH,
            claim_burn_proof_verifier,
        )?;

        let runtime = Runtime::new(Arc::new(runtime_interface));
        let transaction_hash = id.as_hash();

        let instructions = executable.into_instructions();

        let fee_exec_results = Self::process_instructions(&config, &template_provider, &runtime, instructions.fee);

        let fee_exec_result = match fee_exec_results {
            Ok(execution_results) => {
                // Checkpoint the tracker state after the fee instructions have been executed in case of transaction
                // failure.
                if let Err(err) = runtime.interface().set_fee_checkpoint() {
                    let mut finalize = FinalizeResult::new_rejected(transaction_hash, err.to_reject_reason());
                    finalize.execution_results = execution_results;
                    return Ok(ExecuteResult {
                        finalize,
                        execution_time: timer.elapsed(),
                    });
                }
                execution_results
            },
            Err(err) => {
                return Ok(ExecuteResult {
                    finalize: FinalizeResult::new_rejected(
                        transaction_hash,
                        RejectReason::ExecutionFailure(err.to_string()),
                    ),
                    execution_time: timer.elapsed(),
                });
            },
        };

        let instruction_result = Self::process_instructions(&config, &*template_provider, &runtime, instructions.main);

        match instruction_result {
            Ok(execution_results) => {
                let mut finalize = runtime.interface().finalize()?;
                if finalize.fee_receipt.is_paid_in_full() {
                    finalize.execution_results = execution_results;
                } else {
                    finalize.execution_results = fee_exec_result;
                }
                Ok(ExecuteResult {
                    finalize,
                    execution_time: timer.elapsed(),
                })
            },
            // This can happen e.g if you have dangling buckets after running the instructions
            Err(err) => {
                // Reset the state to when the state at the end of the fee instructions. The fee charges for the
                // successful instructions are still charged even though the transaction failed.
                runtime.interface().reset_to_fee_checkpoint()?;
                // Finalize will now contain the fee payments and vault refunds only
                let mut finalize = runtime.interface().finalize()?;
                finalize.execution_results = fee_exec_result;
                finalize.result = TransactionResult::AcceptFeeRejectRest(
                    finalize
                        .result
                        .any_accept()
                        .cloned()
                        .expect("The fee transaction should be there"),
                    RejectReason::ExecutionFailure(err.to_string()),
                );
                Ok(ExecuteResult {
                    finalize,
                    execution_time: timer.elapsed(),
                })
            },
        }
    }

    fn process_instructions(
        config: &TransactionProcessorConfig,
        template_provider: &TTemplateProvider,
        runtime: &Runtime,
        instructions: Vec<Instruction>,
    ) -> Result<Vec<InstructionResult>, TransactionError> {
        let result: Result<_, _> = instructions
            .into_iter()
            .map(|instruction| Self::process_instruction(config, template_provider, runtime, instruction))
            .collect();

        // check that the finalized state is valid
        if result.is_ok() {
            runtime.interface().validate_finalized()?;
        }

        result
    }

    fn process_instruction(
        config: &TransactionProcessorConfig,
        template_provider: &TTemplateProvider,
        runtime: &Runtime,
        instruction: Instruction,
    ) -> Result<InstructionResult, TransactionError> {
        debug!(target: LOG_TARGET, "instruction = {:?}", instruction);
        match instruction {
            Instruction::CreateAccount {
                owner_public_key: public_key_address,
                owner_rule,
                access_rules,
                workspace_id,
            } => Self::create_account(
                template_provider,
                runtime,
                &public_key_address,
                owner_rule,
                access_rules,
                workspace_id,
            ),
            Instruction::CallFunction {
                address: template_address,
                function,
                args,
            } => Self::call_function(template_provider, runtime, &template_address, &function, args),
            Instruction::CallMethod { call, method, args } => {
                Self::call_method(template_provider, runtime, call, &method, args)
            },
            // Basically names an output on the workspace so that you can refer to it as an
            // Arg::Variable
            Instruction::PutLastInstructionOutputOnWorkspace { key } => {
                Self::put_output_on_workspace_with_name(runtime, key)?;
                Ok(InstructionResult::empty())
            },
            Instruction::DropAllProofsInWorkspace => {
                Self::drop_all_proofs_in_workspace(runtime)?;
                Ok(InstructionResult::empty())
            },
            Instruction::EmitLog { level, message } => {
                runtime.interface().emit_log(level, message)?;
                Ok(InstructionResult::empty())
            },
            Instruction::ClaimBurn { claim, output_data } => {
                runtime.interface().claim_burn(*claim, output_data)?;
                Ok(InstructionResult::empty())
            },
            Instruction::ClaimValidatorFees { address } => {
                runtime.interface().claim_validator_fees(address)?;
                Ok(InstructionResult::empty())
            },
            Instruction::AssertBucketContains {
                key,
                resource_address,
                min_amount,
            } => {
                runtime.interface().workspace_invoke(
                    WorkspaceAction::AssertBucketContains,
                    invoke_args![key, resource_address, min_amount].into(),
                )?;
                Ok(InstructionResult::empty())
            },
            Instruction::TakeFromBucket {
                input_bucket,
                amount,
                output_bucket,
            } => {
                let item = runtime
                    .interface()
                    .workspace_invoke(WorkspaceAction::Get, invoke_args![input_bucket].into())?;

                let bucket_ref = BucketRef::Ref(item.decode()?);
                let bucket =
                    runtime
                        .interface()
                        .bucket_invoke(bucket_ref, BucketAction::Take, invoke_args![amount].into())?;
                let prev_bucket_val = runtime
                    .interface()
                    .bucket_invoke(bucket_ref, BucketAction::GetAmount, invoke_args![].into())?
                    .decode::<Amount>()?;
                if prev_bucket_val.is_zero() {
                    // Drop the bucket to prevent a dangling (empty) bucket
                    runtime
                        .interface()
                        .bucket_invoke(bucket_ref, BucketAction::DropEmpty, invoke_args![].into())?;
                }

                runtime
                    .interface()
                    .put_on_workspace(output_bucket, IndexedValue::from_value(bucket.into_value()?)?)?;
                Ok(InstructionResult::empty())
            },
            Instruction::PublishTemplate { binary } => Self::publish_template(config, runtime, binary),
            Instruction::AllocateAddress {
                allocatable_type: substate_type,
                workspace_id,
            } => Self::allocate_address(runtime, substate_type, workspace_id),
            Instruction::StealthTransfer {
                resource_address_ref: resource_address,
                statement,
                revealed_input_bucket,
            } => Self::stealth_transfer(runtime, resource_address, statement, revealed_input_bucket),
            Instruction::PayFee {
                statement,
                revealed_input_bucket,
            } => Self::pay_fee(runtime, statement, revealed_input_bucket),
        }
    }

    fn pay_fee(
        runtime: &Runtime,
        statement: StealthTransferStatement,
        revealed_funds_bucket: Option<WorkspaceOffsetId>,
    ) -> Result<InstructionResult, TransactionError> {
        let revealed_funds_bucket = revealed_funds_bucket
            .map(|id| {
                runtime.resolve_workspace_id(&id).and_then(|r| {
                    r.decode().map_err(|e| RuntimeError::InvalidArgument {
                        argument: "revealed_funds_bucket",
                        reason: format!("Expected workspace id {id} to be a BucketId: {e}"),
                    })
                })
            })
            .transpose()?;
        runtime.interface().pay_fee(statement, revealed_funds_bucket)?;
        Ok(InstructionResult::empty())
    }

    fn stealth_transfer(
        runtime: &Runtime,
        resource_address: ResourceAddressRef,
        statement: StealthTransferStatement,
        revealed_funds_bucket: Option<WorkspaceOffsetId>,
    ) -> Result<InstructionResult, TransactionError> {
        let revealed_funds_bucket = revealed_funds_bucket
            .map(|id| {
                runtime.resolve_workspace_id(&id).and_then(|r| {
                    r.decode().map_err(|e| RuntimeError::InvalidArgument {
                        argument: "revealed_funds_bucket",
                        reason: format!("Expected workspace id {id} to be a BucketId: {e}"),
                    })
                })
            })
            .transpose()?;
        let maybe_bucket = runtime
            .interface()
            .stealth_transfer(resource_address, statement, revealed_funds_bucket)?;
        runtime
            .interface()
            .set_last_instruction_output(IndexedValue::from_type(&maybe_bucket.map(Bucket::from_id))?)?;
        Ok(InstructionResult::empty())
    }

    fn put_output_on_workspace_with_name(runtime: &Runtime, key: WorkspaceId) -> Result<(), TransactionError> {
        runtime
            .interface()
            .workspace_invoke(WorkspaceAction::PutLastInstructionOutput, invoke_args![key].into())?;
        Ok(())
    }

    fn drop_all_proofs_in_workspace(runtime: &Runtime) -> Result<(), TransactionError> {
        runtime
            .interface()
            .workspace_invoke(WorkspaceAction::DropAllProofs, invoke_args![].into())?;
        Ok(())
    }

    /// Allocating a new address for the given [`AllocatableAddressType`].
    fn allocate_address(
        runtime: &Runtime,
        substate_type: AllocatableAddressType,
        workspace_id: WorkspaceId,
    ) -> Result<InstructionResult, TransactionError> {
        let entity_id = runtime.interface().next_entity_id()?;
        let result = runtime
            .interface()
            .allocate_address(substate_type, entity_id, workspace_id)?;

        match result {
            AllocateAddressResult::ComponentAddress(alloc) => Ok(InstructionResult {
                indexed: IndexedValue::from_type(&alloc)?,
                return_type: Type::Other {
                    name: "ComponentAddressAllocation".to_string(),
                },
            }),
            AllocateAddressResult::ResourceAddress(alloc) => Ok(InstructionResult {
                indexed: IndexedValue::from_type(&alloc)?,
                return_type: Type::Other {
                    name: "ResourceAddressAllocation".to_string(),
                },
            }),
        }
    }

    /// Load, validate template binary and adds it to TemplateProvider.
    fn publish_template(
        config: &TransactionProcessorConfig,
        runtime: &Runtime,
        binary: Vec<u8>,
    ) -> Result<InstructionResult, TransactionError> {
        if binary.len() > config.template_binary_max_size_bytes {
            return Err(TransactionError::WasmBinaryTooBig(
                binary.len(),
                config.template_binary_max_size_bytes,
            ));
        }

        // validate binary
        WasmModule::load_template_from_code(&binary)?;
        // creating new substate
        runtime.interface().publish_template(binary)?;

        Ok(InstructionResult::empty())
    }

    fn create_account(
        template_provider: &TTemplateProvider,
        runtime: &Runtime,
        public_key_address: &RistrettoPublicKeyBytes,
        owner_rule: Option<OwnerRule>,
        access_rules: Option<ComponentAccessRules>,
        workspace_id: Option<WorkspaceOffsetId>,
    ) -> Result<InstructionResult, TransactionError> {
        let template = template_provider
            .get_template_module(&ACCOUNT_TEMPLATE_ADDRESS)
            .map_err(|e| TransactionError::FailedToLoadTemplate {
                address: ACCOUNT_TEMPLATE_ADDRESS,
                details: e.to_string(),
            })?
            .ok_or(TransactionError::TemplateNotFound {
                address: ACCOUNT_TEMPLATE_ADDRESS,
            })?;

        let function_def = template
            .template_def()
            .get_function(ACCOUNT_CONSTRUCTOR_FUNCTION)
            .cloned()
            .ok_or_else(|| TransactionError::FunctionNotFound {
                name: ACCOUNT_CONSTRUCTOR_FUNCTION.to_string(),
            })?;

        let account_address = derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, public_key_address);

        // the public key is the first argument of the Account template constructor
        let mut args = call_args![
            NonFungibleAddress::from_public_key(*public_key_address),
            owner_rule,
            access_rules
        ];

        // add the optional workspace bucket with the initial funds of the account
        if let Some(workspace_id) = workspace_id {
            args.push(call_arg![WorkspaceOffset(workspace_id)]);
        } else {
            let none: Option<Bucket> = None;
            args.push(call_arg![Literal(none)]);
        }

        let resolved_args = runtime.resolve_args(&args)?;
        let arg_scope = resolved_args
            .iter()
            .map(IndexedWellKnownTypes::from_value)
            .collect::<Result<_, _>>()?;

        runtime.interface().push_call_frame(PushCallFrame::Static {
            template_address: ACCOUNT_TEMPLATE_ADDRESS,
            module_name: template.template_name().to_string(),
            arg_scope,
            entity_id: account_address.entity_id(),
        })?;

        let result = Self::invoke_template(template, runtime.clone(), function_def, resolved_args)?;

        runtime.interface().validate_return_value(&result.indexed)?;

        runtime.interface().pop_call_frame()?;

        Ok(result)
    }

    pub(crate) fn call_function(
        template_provider: &TTemplateProvider,
        runtime: &Runtime,
        template_address: &TemplateAddress,
        function: &str,
        args: Vec<InstructionArg>,
    ) -> Result<InstructionResult, TransactionError> {
        let template = template_provider
            .get_template_module(template_address)
            .map_err(|e| TransactionError::FailedToLoadTemplate {
                address: *template_address,
                details: e.to_string(),
            })?
            .ok_or(TransactionError::TemplateNotFound {
                address: *template_address,
            })?;

        let function_def = template.template_def().get_function(function).cloned().ok_or_else(|| {
            TransactionError::FunctionNotFound {
                name: function.to_string(),
            }
        })?;

        let resolved_args = runtime.resolve_args(&args)?;
        let arg_scope = resolved_args
            .iter()
            .map(IndexedWellKnownTypes::from_value)
            .collect::<Result<_, _>>()?;

        runtime.interface().push_call_frame(PushCallFrame::Static {
            template_address: *template_address,
            module_name: template.template_name().to_string(),
            arg_scope,
            entity_id: runtime.interface().next_entity_id()?,
        })?;

        let result = Self::invoke_template(template, runtime.clone(), function_def, resolved_args)?;

        runtime.interface().validate_return_value(&result.indexed)?;

        runtime.interface().pop_call_frame()?;

        Ok(result)
    }

    pub(crate) fn call_method(
        template_provider: &TTemplateProvider,
        runtime: &Runtime,
        call: ComponentCall,
        method: &str,
        args: Vec<InstructionArg>,
    ) -> Result<InstructionResult, TransactionError> {
        let (component_address, component) = runtime.interface().load_component(call)?;
        let template_address = component.template_address;

        let template = template_provider
            .get_template_module(&template_address)
            .map_err(|e| TransactionError::FailedToLoadTemplate {
                address: template_address,
                details: e.to_string(),
            })?
            .ok_or(TransactionError::TemplateNotFound {
                address: template_address,
            })?;

        let function_def = template.template_def().get_function(method).cloned().ok_or_else(|| {
            TransactionError::FunctionNotFound {
                name: method.to_string(),
            }
        })?;

        let lock_flag = if function_def.is_mut {
            LockFlag::Write
        } else {
            LockFlag::Read
        };

        let component_lock = runtime.interface().lock_component(component_address, lock_flag)?;

        let resolved_args = runtime.resolve_args(&args)?;
        let arg_scope = resolved_args
            .iter()
            .map(IndexedWellKnownTypes::from_value)
            .collect::<Result<_, _>>()?;

        let component_scope = IndexedWellKnownTypes::from_value(component.state())?;

        runtime.interface().push_call_frame(PushCallFrame::ForComponent {
            template_address,
            module_name: template.template_name().to_string(),
            component_scope,
            component_lock: component_lock.clone(),
            arg_scope: Box::new(arg_scope),
            entity_id: component.entity_id,
        })?;

        // This must come after the call frame as that defines the authorization scope
        runtime
            .interface()
            .check_component_access_rules(method, &component_lock)?;

        let mut final_args = Vec::with_capacity(resolved_args.len() + 1);
        final_args.push(to_value(&component_address)?);
        final_args.extend(resolved_args);

        let result = Self::invoke_template(template, runtime.clone(), function_def, final_args)?;

        runtime.interface().validate_return_value(&result.indexed)?;
        runtime.interface().pop_call_frame()?;

        Ok(result)
    }

    fn invoke_template(
        module: LoadedTemplate,
        runtime: Runtime,
        function_def: FunctionDef,
        args: Vec<tari_bor::Value>,
    ) -> Result<InstructionResult, TransactionError> {
        let result = match module {
            LoadedTemplate::Wasm(loaded) => {
                let mut store = loaded.create_store();
                let mut process = WasmProcess::init(&mut store, loaded, runtime)?;
                process.invoke(&mut store, &function_def, args)?
            },
        };
        Ok(result)
    }
}
