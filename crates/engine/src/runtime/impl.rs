//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use log::{warn, *};
use tari_bor::{MaybeTagged, decode_exact};
use tari_crypto::{ristretto::RistrettoPublicKey, tari_utilities::ByteArray};
use tari_engine_types::{
    bucket::Bucket,
    commit_result::{FeeIntentFallback, FinalizeResult, RejectReason},
    confidential::{ClaimedOutputTombstone, MinotariBurnClaimProof},
    crypto,
    entity_id_provider::EntityIdProvider,
    events::Event,
    fees::FeeReceipt,
    hashing::hash_template_code,
    indexed_value::{IndexedValue, IndexedWellKnownTypes},
    limits,
    limits::{CompileCounts, ModuleShape},
    lock::LockFlag,
    logs::LogEntry,
    published_template::{PublishedTemplate, PublishedTemplateAddress, TemplateBlob},
    resource::Resource,
    resource_container::{ResourceContainer, ResourceError},
    stealth,
    substate::{SubstateId, SubstateValue},
    vault::Vault,
};
use tari_ootle_common_types::services::template_provider::TemplateProvider;
use tari_ootle_transaction::{
    AllocatableAddressType,
    Assertion,
    ComponentReference,
    ResourceAddressRef,
    args::{InstructionArg, WorkspaceId, WorkspaceOffsetId},
};
use tari_template_abi::{FunctionDef, FunctionKind, TemplateDef, Type, WellKnownType};
use tari_template_builtin::{ACCOUNT_TEMPLATE_ADDRESS, NFT_FAUCET_TEMPLATE_ADDRESS, is_builtin_template_address};
use tari_template_lib::{
    SpendContext,
    args::{
        AddressAllocationInvokeArg,
        AllocateAddressResult,
        BucketAction,
        BucketGetAmountArg,
        BucketRef,
        BuiltinTemplateAction,
        BurnBucketArg,
        BurnStealthUtxoArg,
        CallAction,
        CallFunctionArg,
        CallMethodArg,
        CallerContextAction,
        ComponentAction,
        ComponentRef,
        ConsensusAction,
        CreateComponentArg,
        CreateResourceArg,
        FreezeResourceArg,
        GenerateRandomAction,
        InvokeResult,
        MintArg,
        MintResourceArg,
        NonFungibleAction,
        PayFeeArg,
        ProofAction,
        ProofRef,
        RecallResourceArg,
        ResourceAction,
        ResourceGetNonFungibleArg,
        ResourceRef,
        ResourceUpdateNonFungibleDataArg,
        SetFreezeConfidentialOutputsArg,
        SetFreezeStealthUtxosArg,
        SpendContextAction,
        StealthTransferResourceArg,
        UpdateAccessRuleArg,
        UpdateAuthHookArg,
        VaultAction,
        VaultCreateProofByFungibleAmountArg,
        VaultCreateProofByNonFungiblesArg,
        VaultFreezeFlag,
        VaultWithdrawArg,
        WorkspaceAction,
    },
    invoke_args,
    models::{BucketId, ComponentAddressAllocation, NonFungible, NotAuthorized, ResourceAddressAllocation, VaultRef},
    template::BuiltinTemplate,
    types::{
        Amount,
        AuthHook,
        AuthHookCaller,
        ClaimedOutputTombstoneAddress,
        ComponentAddress,
        ConfidentialOutputAddress,
        EntityId,
        Hash32,
        LogLevel,
        Metadata,
        NonFungibleAddress,
        OwnerRule,
        ResourceAddress,
        ResourceInfo,
        ResourceType,
        SubstateOwnerRule,
        TemplateAddress,
        TemplateMetadataHash,
        UtxoAddress,
        ValidatorFeePoolAddress,
        VaultId,
        access_rules::{
            AccessRule,
            ComponentAccessRules,
            InvalidAccessRule,
            RequireRule,
            ResourceAuthAction,
            RestrictedAccessRule,
            RuleRequirement,
            UpdateRule,
        },
        bytes::Bytes,
        constants::{TARI_TOKEN, TOKEN_SYMBOL},
        crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, ValueKnowledgeProof},
        engine_args::IntrinsicId,
        stealth::{
            AtomicCondition,
            BuiltinPredicate,
            Covenant,
            CurrentInputView,
            MerkleProof,
            SpendCondition,
            SpendWitness,
            StealthInput,
            StealthTransferStatement,
            TemplateFunction,
        },
        try_metadata,
    },
};

use super::{
    ActionIdent,
    NativeAction,
    Runtime,
    RuntimeEvent,
    spend_script_execution::{SpendScriptExecution, StatementSpendView},
    working_state::{WorkingState, reject_transient_values_in},
};
use crate::{
    intrinsics,
    runtime::{
        LoadedComponent,
        RuntimeError,
        RuntimeInterface,
        engine_args::{EngineArgChargeError, EngineArgs},
        error::{ArgumentValidationError, LimitError},
        locking::{LockError, LockedSubstate},
        pay_fee::PayFee,
        scope::{FrameWriteMode, PushCallFrame},
        tracker::{ComputeAllowance, FinalizedState, StateTracker},
    },
    state_store::StateReader,
    template::LoadedTemplate,
    traits::{ClaimProofError, ClaimProofRejection, ClaimProofVerifier},
    transaction::{ModulesCollection, TransactionProcessor},
    wasm::WasmInstanceCache,
};

const LOG_TARGET: &str = "tari::ootle::engine::runtime::impl";

/// The engine's host interface for one transaction.
///
/// A nested call runs against the same execution state as the frame that made it, so everything
/// mutable here is shared rather than owned: [`Self::for_nested_call`] hands out another interface
/// over the same state, and the caller never has to keep a `&mut` alive across the re-entry.
pub struct RuntimeInterfaceImpl<TStore, TTemplateProvider> {
    tracker: Rc<StateTracker<TStore>>,
    template_provider: Arc<TTemplateProvider>,
    entity_id_provider: Rc<RefCell<EntityIdProvider>>,
    /// `None` when the transaction declares no main signer.
    main_signer: Option<RistrettoPublicKeyBytes>,
    modules: ModulesCollection<TStore>,
    claim_burn_proof_verifier: Arc<dyn ClaimProofVerifier + Send + Sync + 'static>,
    /// Transaction blob payloads, immutable for the duration of execution. Used to resolve
    /// `InstructionArg::Blob(idx)` references against the surrounding transaction's blobs.
    blobs: Rc<tari_ootle_transaction::Blobs>,
    /// The introspection context made available to a spend-script predicate for the duration of its evaluation. It is
    /// set immediately before invoking the predicate and cleared immediately after, so `spend_context_invoke` — which
    /// the predicate reaches through its own nested interface — can serve the `SpendContext` accessors.
    spend_exec_context: Rc<RefCell<Option<SpendScriptExecution>>>,
    /// The template instances this transaction has created, shared by every frame's interface.
    wasm_instances: Rc<RefCell<WasmInstanceCache>>,
}

impl<TStore, TTemplateProvider> Clone for RuntimeInterfaceImpl<TStore, TTemplateProvider> {
    fn clone(&self) -> Self {
        Self {
            tracker: self.tracker.clone(),
            template_provider: self.template_provider.clone(),
            entity_id_provider: self.entity_id_provider.clone(),
            main_signer: self.main_signer,
            modules: self.modules.clone(),
            claim_burn_proof_verifier: self.claim_burn_proof_verifier.clone(),
            blobs: self.blobs.clone(),
            spend_exec_context: self.spend_exec_context.clone(),
            wasm_instances: self.wasm_instances.clone(),
        }
    }
}

impl<TStore: StateReader + Clone + 'static, TTemplateProvider: TemplateProvider<Template = LoadedTemplate>>
    RuntimeInterfaceImpl<TStore, TTemplateProvider>
{
    pub(crate) fn initialize(
        tracker: StateTracker<TStore>,
        template_provider: Arc<TTemplateProvider>,
        main_signer: Option<RistrettoPublicKeyBytes>,
        entity_id_provider: EntityIdProvider,
        modules: ModulesCollection<TStore>,
        claim_burn_proof_verifier: Arc<dyn ClaimProofVerifier + Send + Sync + 'static>,
        blobs: Rc<tari_ootle_transaction::Blobs>,
    ) -> Result<Self, RuntimeError> {
        let runtime = Self {
            tracker: Rc::new(tracker),
            template_provider,
            entity_id_provider: Rc::new(RefCell::new(entity_id_provider)),
            main_signer,
            modules,
            claim_burn_proof_verifier,
            blobs,
            spend_exec_context: Rc::new(RefCell::new(None)),
            wasm_instances: Rc::new(RefCell::new(WasmInstanceCache::new())),
        };
        runtime.invoke_modules_on_initialize()?;
        Ok(runtime)
    }

    fn main_signer(&self, needed_for: &'static str) -> Result<RistrettoPublicKeyBytes, RuntimeError> {
        self.main_signer.ok_or(RuntimeError::NoMainSigner { needed_for })
    }

    fn invoke_modules_on_initialize(&self) -> Result<(), RuntimeError> {
        for module in self.modules.iter() {
            module.on_initialize(&self.tracker)?;
        }
        Ok(())
    }

    fn invoke_modules_on_runtime_call(&self, function: &'static str) -> Result<(), RuntimeError> {
        // Core sandbox enforcement runs first and unconditionally. It deliberately does NOT live in a
        // RuntimeModule: modules are optional, observer-style functionality (fees, call tracking), so making a
        // security invariant depend on one would mean dropping that module silently re-opens the sandbox. This is the
        // single per-host-op entry point, so enforcing here covers every op that routes through it regardless of
        // which modules are registered.
        self.enforce_frame_restrictions(function)?;
        for module in self.modules.iter() {
            module.on_runtime_call(&self.tracker, function)?;
        }
        Ok(())
    }

    /// Layer (b) of the frame sandbox: deny the effectful or non-deterministic host ops that are NOT mediated by the
    /// write-lock chokepoint (layer (a) in `WorkingState::try_lock` / `new_substate`, which neutralises every state
    /// write, and `WorkingState::check_create_allowed`, which refuses new buckets, proofs and address allocations).
    /// Together they make a spend-script predicate provably side-effect-free and deterministic,
    /// and confine a resource auth hook to its own component state.
    ///
    /// Events are permitted in both modes: an event is an output of execution that no later code can observe, and it
    /// is discarded with the transaction if the frame fails, so it is neither a side effect on state nor a source of
    /// non-determinism.
    ///
    /// The lists contain WASM host ops (each backed by an `EngineOp`) as well as host-initiated re-entry into WASM
    /// (`invoke_resource_access_hook` and `evaluate_spend_script`), which must be refused before it runs because it
    /// happens before any write lock. Instruction-level operations such as `pay_fee` and `publish_template` have no
    /// `EngineOp`, run only at the top level, and so can never execute in a restricted context. `call_invoke` is
    /// also blocked at the frame level (`allow_cross_template_calls == false`) and listed here for defence in depth.
    /// `allocate_address_invoke` is also refused by `WorkingState::new_address_allocation`; listing it here refuses
    /// the op before it draws an address from the id provider.
    fn enforce_frame_restrictions(&self, function: &'static str) -> Result<(), RuntimeError> {
        const FORBIDDEN_IN_READ_ONLY: &[&str] = &[
            "call_invoke",
            "invoke_resource_access_hook",
            "evaluate_spend_script",
            "generate_random_invoke",
            "generate_uuid",
            "proof_invoke",
            "bucket_invoke",
            "allocate_address_invoke",
        ];
        const FORBIDDEN_IN_OWN_COMPONENT: &[&str] = &[
            "call_invoke",
            "invoke_resource_access_hook",
            "evaluate_spend_script",
            "proof_invoke",
            "bucket_invoke",
            "allocate_address_invoke",
        ];

        match self.tracker.current_frame_write_mode() {
            FrameWriteMode::Full => Ok(()),
            FrameWriteMode::OwnComponent if FORBIDDEN_IN_OWN_COMPONENT.contains(&function) => {
                Err(RuntimeError::ForbiddenInRestrictedFrame {
                    operation: function,
                    frame: self.tracker.current_restricted_frame(),
                })
            },
            FrameWriteMode::ReadOnly if FORBIDDEN_IN_READ_ONLY.contains(&function) => {
                Err(RuntimeError::ForbiddenInReadOnlyContext { operation: function })
            },
            FrameWriteMode::OwnComponent | FrameWriteMode::ReadOnly => Ok(()),
        }
    }

    fn invoke_modules_on_before_finalize(&self) -> Result<(), RuntimeError> {
        for module in self.modules.iter() {
            module.on_before_finalize(&self.tracker)?;
        }
        Ok(())
    }

    /// Settles the transaction into a result.
    ///
    /// The fee module charges twice here, and the order is the point of the split. It first charges
    /// against the working state: those charges are what [`StateTracker::select_finalized_state`] tests
    /// against the payments, so they decide whether the transaction commits or falls back to a
    /// fee-intent commit. Once that is decided, it charges again against the state actually chosen,
    /// which on a fee-intent commit holds only what the fee intent touched. A transaction is
    /// therefore gated on the cost of the state it asked to commit, but pays for the state that is
    /// really persisted.
    fn finalize_with(
        &self,
        failure: Option<RejectReason>,
        with_fee_intent_fallback: bool,
    ) -> Result<(FinalizeResult, Option<FeeIntentFallback>), RuntimeError> {
        self.invoke_modules_on_before_finalize()?;
        let mut finalized = self.tracker.select_finalized_state(failure)?;
        // A commit persists the very state the first pass charged against, so charging it again
        // would recompute the same numbers from the same inputs. Only a fee-intent commit swaps the
        // state out from under those charges, and only it needs them redone.
        if !finalized.outcome().is_commit() {
            self.invoke_modules_on_before_persist(&mut finalized)?;
        }
        // The fallback is optional, so failing to settle it leaves the transaction to commit in full or abort as
        // it would without one, rather than failing a result that is already settled.
        let fallback = if with_fee_intent_fallback {
            self.finalize_fee_intent_fallback(&finalized).unwrap_or_else(|err| {
                warn!(target: LOG_TARGET, "Failed to settle the fee-intent fallback: {err}");
                None
            })
        } else {
            None
        };
        Ok((self.tracker.finalize(finalized)?, fallback))
    }

    /// Settles the fee-intent commit of `committed` the way [`Self::finalize_with`] settles a main-intent failure, so
    /// that it is charged identically to one.
    fn finalize_fee_intent_fallback(
        &self,
        committed: &FinalizedState<TStore>,
    ) -> Result<Option<FeeIntentFallback>, RuntimeError> {
        let Some(mut fallback) = self.tracker.select_fee_intent_fallback(committed) else {
            return Ok(None);
        };
        self.invoke_modules_on_before_persist(&mut fallback)?;
        let result = self.tracker.finalize(fallback)?;
        Ok(FeeIntentFallback::from_fee_intent_finalization(result))
    }

    fn invoke_modules_on_fee_checkpoint(&self) -> Result<(), RuntimeError> {
        for module in self.modules.iter() {
            self.tracker
                .with_chargeable_state(|state| module.on_fee_checkpoint(state))?;
        }
        Ok(())
    }

    fn invoke_modules_on_before_persist(&self, finalized: &mut FinalizedState<TStore>) -> Result<(), RuntimeError> {
        for module in self.modules.iter() {
            module.on_before_persist(&mut finalized.chargeable_state())?;
            if let Some(mut fee_intent_state) = finalized.fee_intent_chargeable_state() {
                module.on_before_persist(&mut fee_intent_state)?;
            }
        }
        Ok(())
    }

    fn invoke_modules_on_runtime_event(&self, event: RuntimeEvent) -> Result<(), RuntimeError> {
        for module in self.modules.iter() {
            module.on_runtime_event(&self.tracker, &event)?;
        }
        Ok(())
    }

    pub fn get_template_def(&self, template_address: &TemplateAddress) -> Result<Arc<TemplateDef>, RuntimeError> {
        let loaded = self
            .template_provider
            .get_template(template_address)
            .map_err(|e| RuntimeError::FailedToLoadTemplate {
                address: *template_address,
                details: e.to_string(),
            })?
            .ok_or(RuntimeError::TemplateNotFound {
                template_address: *template_address,
            })?;
        self.track_template_loaded(template_address, loaded.code_size())?;

        Ok(loaded.shared_template_def())
    }

    fn validate_return_value(&self, value: &IndexedValue) -> Result<(), RuntimeError> {
        self.tracker.read_with(|state| {
            for bucket_id in value.bucket_ids() {
                let _ignore = state.get_bucket(*bucket_id)?;
            }

            // `get_bucket` scope-checks; `get_proof` does not, so the frame's own scope is checked here to keep a
            // returned proof to the same rule as a returned bucket: only a held proof crosses to the caller.
            for proof_id in value.proof_ids() {
                state.current_call_scope()?.require_held_proof(proof_id)?;
                let _ignore = state.get_proof(*proof_id)?;
            }

            for id in value.referenced_substates() {
                if !state.substate_exists(&id)? {
                    debug!(
                        target: LOG_TARGET,
                        "Returned substate {id} does not exist",
                    );

                    return Err(RuntimeError::NonExistentSubstateReturned { id });
                }
            }

            Ok(())
        })
    }

    /// Metadata a template supplies is persisted and served as JSON by every node, wallet and indexer
    /// that reads it. A value's JSON form is that of the [`tari_bor::Value`] it decodes to, so a value
    /// that does not decode to one — a CBOR simple value, or nesting past
    /// [`tari_bor::MAX_DECODE_DEPTH`] — would commit and then fail every reader.
    fn check_metadata_values(argument: &'static str, metadata: &Metadata) -> Result<(), RuntimeError> {
        for (key, value) in metadata {
            if let Err(e) = tari_bor::check_value_form(value.as_bytes()) {
                return Err(RuntimeError::InvalidArgument {
                    argument,
                    reason: format!("metadata value \"{key}\" is not a representable CBOR value: {e}"),
                });
            }
        }
        Ok(())
    }

    fn check_token_symbol(metadata: &Metadata) -> Result<(), RuntimeError> {
        if !metadata.contains_key(TOKEN_SYMBOL) {
            return Ok(());
        }
        let Some(symbol) = metadata.get_str(TOKEN_SYMBOL) else {
            return Err(RuntimeError::InvalidArgument {
                argument: "metadata",
                reason: "token symbol must be a text string".to_string(),
            });
        };
        if symbol.len() > limits::MAX_TOKEN_SYMBOL_LEN {
            return Err(RuntimeError::InvalidArgument {
                argument: "metadata",
                reason: format!(
                    "token symbol exceeds {} bytes (got {})",
                    limits::MAX_TOKEN_SYMBOL_LEN,
                    symbol.len()
                ),
            });
        }
        Ok(())
    }

    /// Emits one of the engine's builtin `std.*` events.
    ///
    /// Events are persisted in the transaction receipt and priced by their encoded length, so a
    /// `payload` entry is paid for by every caller of the instruction that emits it. A payload must
    /// therefore carry only what a reader cannot recover from the substates the same transaction
    /// ups: the amount moved, which rule changed, which vault was reached. Anything the reader can
    /// read off `substate_id` or off a substate it names — a resource's type, symbol or metadata —
    /// is charged for twice and must stay out. A consumer that needs a substate's contents to
    /// interpret an event resolves it once and caches it — a vault's resource, for instance, is
    /// fixed for the vault's life — rather than having every caller pay to carry it.
    fn emit_std_event<T: Into<SubstateId>>(
        object_name: &str,
        action: &str,
        substate_id: T,
        payload: Metadata,
        state_mut: &mut WorkingState<TStore>,
    ) -> Result<(), RuntimeError> {
        let template_address = *state_mut.current_template()?;
        let event = Event::std(Some(substate_id.into()), template_address, object_name, action, payload);
        trace!(target: LOG_TARGET, "Emitted event {}", event);
        state_mut.push_event(event)?;
        Ok(())
    }

    fn invoke_resource_access_hook(
        &self,
        auth_hook: AuthHook,
        mut auth_caller: AuthHookCaller,
        action: ResourceAuthAction,
    ) -> Result<(), RuntimeError> {
        self.invoke_modules_on_runtime_call("invoke_resource_access_hook")?;
        let skip_hook = self.tracker.read_with(|state| {
            let current_component = auth_caller.component().copied();
            // Only execute hooks if the resource is being acted upon by an external component
            if current_component == Some(auth_hook.component_address) {
                return Ok::<_, RuntimeError>(true);
            }
            // The hook component may still be an unused address allocation when the resource is acted upon in the
            // same call that creates it. For example, commonly a user creates a resource with initial supply and
            // deposits it into a vault before creating the component. In this case, we "skip" the hook. Only the
            // frame holding the allocation, running the template that made it, may skip.
            if let Some(alloc) = state.get_allocated_address_in_current_scope(auth_hook.component_address)? &&
                alloc.template_address() == state.current_template().ok()
            {
                return Ok(true);
            }
            // Any other hook component already exists on the network, and execution can only read it if the
            // transaction declared it as an input.
            if !state.store().exists(&auth_hook.component_address.into())? {
                return Err(RuntimeError::AuthHookComponentNotDeclared {
                    resource_address: *auth_caller.resource(),
                    component_address: auth_hook.component_address,
                });
            }
            Ok(false)
        })?;

        if skip_hook {
            return Ok(());
        }

        // The hook frame's scope comes from the arguments as they stand before the acting component's state is
        // attached, so the hook receives that state as data only. The acting component's frame validated the state
        // when it wrote it, and the substates it names belong to the acting component: a root substate among them
        // may be absent from this transaction, and none of them enter the hook frame's scope.
        let hook_arg_scope = [
            IndexedWellKnownTypes::from_value(&tari_bor::to_value(&action)?)?,
            IndexedWellKnownTypes::from_value(&tari_bor::to_value(&auth_caller)?)?,
        ]
        .into_iter()
        .collect();

        if let Some(component) = auth_caller.component().copied() {
            self.invoke_modules_on_runtime_call("load_component")?;
            self.charge_component_load(component)?;
            let state = self.tracker.write_with(|state_mut| {
                Ok::<_, RuntimeError>(state_mut.load_and_cache_component(component)?.state().clone())
            })?;
            auth_caller.with_component_state(state);
        }

        let resource_id: SubstateId = (*auth_caller.resource()).into();
        // The signature of a call back is (action: ResourceAuthAction, auth_caller: AuthHookCaller).
        let args = invoke_args![action, auth_caller];

        // The hook frame only accepts the `AuthHookCaller` argument if the resource it names is in the acting
        // frame's scope. The reference is scoped to the hook call: the acting frame's scope must be the same after
        // the hook as before it, whether or not the resource has a hook.
        let resource_was_in_scope = self.tracker.write_with(|state_mut| {
            let scope = state_mut.current_call_scope_mut()?;
            let was_in_scope = scope.is_substate_in_scope(&resource_id);
            if !was_in_scope {
                scope.add_substate_to_referenced(resource_id.clone());
            }
            Ok::<_, RuntimeError>(was_in_scope)
        })?;

        // The acting component never chose the hook code, so the hook's frame is confined to its own component
        // state: it cannot act on any vault or resource it was handed, nor call out to a frame that could.
        let ret = self.invoke_component_method(
            auth_hook.component_address,
            &auth_hook.method,
            args,
            Some(FrameWriteMode::OwnComponent),
            Some(hook_arg_scope),
        );
        if !resource_was_in_scope {
            self.tracker.write_with(|state_mut| {
                state_mut
                    .current_call_scope_mut()?
                    .remove_substate_from_referenced(&resource_id);
                Ok::<_, RuntimeError>(())
            })?;
        }
        let ret = ret.map_err(|e| match e {
            RuntimeError::CrossTemplateCallMethodError { details, .. } => RuntimeError::AccessDeniedAuthHook {
                action_ident: action.into(),
                details: details.to_string(),
            },
            _ => e,
        })?;
        // Enforce that the return type is actually empty. We cannot rely on the function's declared return type
        // because that comes from the template definition which is defined by the template author and may not reflect
        // actual behaviour. `is_unit` accepts either `Value::Null` (ciborium/serde encoding of `()`) or
        // `Value::Array([])` (minicbor encoding of `()`).
        if !ret.value().is_unit() {
            return Err(RuntimeError::UnexpectedNonNullInAuthHookReturn);
        }

        Ok(())
    }

    /// Authorizes `action` on a resource against its static access rule and then its auth hook. `validate` checks
    /// the resource is one the action applies to. The resource is unlocked before the hook runs, so the hook may
    /// read it.
    fn authorize_resource_action<F>(
        &self,
        resource_address: ResourceAddress,
        action: ResourceAuthAction,
        validate: F,
    ) -> Result<(), RuntimeError>
    where
        F: FnOnce(&Resource) -> Result<(), RuntimeError>,
    {
        let (maybe_auth_hook, auth_caller) = self.tracker.write_with_authorization(|state_mut| {
            let resource_lock = state_mut.read_lock_substate(SubstateId::Resource(resource_address))?;
            let resource = state_mut.get_resource(&resource_lock)?;
            validate(resource)?;

            state_mut.authorization().check_resource_access_rules(
                action,
                resource.as_ownership(),
                resource.access_rules(),
            )?;

            let auth_hook = resource.auth_hook().cloned();
            let auth_caller = state_mut.get_auth_caller(&resource_lock)?;

            state_mut.unlock_substate(resource_lock)?;
            Ok::<_, RuntimeError>((auth_hook, auth_caller))
        })?;

        if let Some(auth_hook) = maybe_auth_hook {
            self.invoke_resource_access_hook(auth_hook, auth_caller, action)?;
        }
        Ok(())
    }

    /// Authorizes `action` on a vault against its freeze flags and its resource's static access rule and auth hook.
    /// Both the vault and resource are unlocked before the hook runs, so the hook may read either substate.
    fn authorize_vault_action(
        &self,
        vault_id: VaultId,
        action: ResourceAuthAction,
        freeze_flag: VaultFreezeFlag,
    ) -> Result<ResourceAddress, RuntimeError> {
        let resource_address = self.tracker.write_with(|state_mut| {
            let vault_lock = state_mut.write_lock_substate(SubstateId::Vault(vault_id))?;
            let vault = state_mut.get_vault(&vault_lock)?;
            let freeze_flags = vault.freeze_flags();
            let resource_address = *vault.resource_address();
            state_mut.unlock_substate(vault_lock)?;

            if freeze_flags.contains(freeze_flag) {
                return Err(RuntimeError::VaultFrozen { vault_id, freeze_flag });
            }
            Ok::<_, RuntimeError>(resource_address)
        })?;

        self.authorize_resource_action(resource_address, action, |_| Ok(()))?;
        Ok(resource_address)
    }

    /// An interface over this transaction's state for a nested call to run against.
    ///
    /// The nested frame shares the state, not the handle: every frame gets its own interface, so no
    /// frame holds a borrow of another's while its call is in flight.
    fn for_nested_call(&self) -> Runtime {
        Runtime::new(Rc::new(self.clone()))
    }

    /// Calls `method` on `component_address`. `arg_scope`, when given, is the callee frame's argument scope in
    /// place of the one derived from `args`.
    fn invoke_component_method(
        &self,
        component_address: ComponentAddress,
        method: &str,
        args: Vec<Bytes>,
        restrict_frame_to: Option<FrameWriteMode>,
        arg_scope: Option<IndexedWellKnownTypes>,
    ) -> Result<IndexedValue, RuntimeError> {
        let call_runtime = self.for_nested_call();

        TransactionProcessor::<TStore, _>::call_method(
            &*self.template_provider,
            &call_runtime,
            component_address.into(),
            method,
            args.into_iter().map(InstructionArg::Literal).collect(),
            restrict_frame_to,
            arg_scope,
        )
        .map_err(|e| RuntimeError::CrossTemplateCallMethodError {
            component_address,
            method: method.to_string(),
            details: Box::new(e),
        })
    }

    fn invoke_template_function(
        &self,
        template_address: &TemplateAddress,
        function: &str,
        args: Vec<InstructionArg>,
        restrict_frame_to: Option<FrameWriteMode>,
    ) -> Result<IndexedValue, RuntimeError> {
        let call_runtime = self.for_nested_call();

        TransactionProcessor::<TStore, _>::call_function(
            &*self.template_provider,
            &call_runtime,
            template_address,
            function,
            args,
            restrict_frame_to,
        )
        .map_err(|e| RuntimeError::CrossTemplateCallFunctionError {
            template_address: *template_address,
            function: function.to_string(),
            details: Box::new(e),
        })
    }

    /// Takes a resource's lock back after its auth hook has run. A hook must be able to read the resource it
    /// guards, so the lock cannot be held across the call; the hook frame cannot write, so what it reads is what
    /// the operation goes on to alter. Only an operation that alters the resource takes a write lock, so that a
    /// transaction may declare the resource as a read input when it does not.
    fn relock_resource(
        &self,
        resource_address: ResourceAddress,
        lock_flag: LockFlag,
    ) -> Result<LockedSubstate, RuntimeError> {
        self.tracker.write_with(|state_mut| match lock_flag {
            LockFlag::Read => state_mut.read_lock_substate(SubstateId::Resource(resource_address)),
            LockFlag::Write => state_mut.write_lock_substate(SubstateId::Resource(resource_address)),
        })
    }

    /// A bucket with funds locked by a proof may only be held, never consumed. Every operation that empties one —
    /// deposit, burn, join, fee payment — operates on the unlocked funds alone, so consuming a locked bucket would
    /// destroy the locked portion while the proof still points at it.
    fn check_bucket_is_unlocked(op: &'static str, bucket_id: BucketId, bucket: &Bucket) -> Result<(), RuntimeError> {
        if bucket.has_locked_funds() {
            return Err(RuntimeError::InvalidOpLockedBucket {
                op,
                bucket_id,
                locked_amount: bucket.locked_amount(),
            });
        }
        Ok(())
    }

    /// Charges the native verification cost of a confidential mint against the payment-funded allowance before any
    /// of its proof crypto runs. The charged work must match what `WorkingState::mint_resource` goes on to verify:
    /// the value proof is only checked when the resource tracks supply and the statement mints a commitment.
    fn charge_confidential_mint(
        &self,
        mint_arg: &MintArg,
        has_view_key: bool,
        is_total_supply_tracking_enabled: bool,
    ) -> Result<(), RuntimeError> {
        let MintArg::Confidential {
            statement,
            value_proofs,
        } = mint_arg
        else {
            return Ok(());
        };

        self.tracker
            .charge_native_execution(tari_engine_types::confidential::statement_native_points(
                statement,
                has_view_key,
            ))?;

        if is_total_supply_tracking_enabled {
            // One proof is verified per minted commitment; the price depends on each proof's variant.
            let points = statement
                .output
                .iter()
                .filter_map(|output| value_proofs.get(&output.commitment))
                .map(crypto::value_proof_native_points)
                .fold(0u64, u64::saturating_add);
            if points > 0 {
                self.tracker.charge_native_execution(points)?;
            }
        }

        Ok(())
    }

    /// Validates that `hook` names a method with an authorization hook's signature. `argument` names the engine
    /// argument the hook arrived in, so that a rejection points at the call the caller actually made.
    fn check_resource_auth_hook(&self, argument: &'static str, hook: &AuthHook) -> Result<(), RuntimeError> {
        let template_address = self
            .tracker
            .write_with(|state| state.get_template_for_component(hook.component_address))?;
        let template = self.get_template_def(&template_address)?;
        let func = template
            .get_function(&hook.method)
            .ok_or(RuntimeError::InvalidArgument {
                argument,
                reason: format!("Authorize hook '{}' not found", hook),
            })?;

        if !matches!(func.output, Type::Unit) {
            return Err(RuntimeError::InvalidArgument {
                argument,
                reason: format!("Authorize hook '{}' must return unit", hook),
            });
        }

        if !func.has_receiver() {
            return Err(RuntimeError::InvalidArgument {
                argument,
                reason: format!("Authorize hook '{}' must be a method taking &self or &mut self", hook),
            });
        }

        if func.arguments.len() != 2 {
            return Err(RuntimeError::InvalidArgument {
                argument,
                reason: format!(
                    "Authorize hook '{}' must take 2 arguments after self, but found {}",
                    hook,
                    func.arguments.len()
                ),
            });
        }

        if !func
            .arguments
            .first()
            .expect("length checked")
            .arg_type
            .is_well_known(WellKnownType::ResourceAuthAction)
        {
            return Err(RuntimeError::InvalidArgument {
                argument,
                reason: format!("Authorize hook '{}' must take a ResourceAuthAction as argument 1", hook),
            });
        }

        if !func
            .arguments
            .get(1)
            .expect("length checked")
            .arg_type
            .is_well_known(WellKnownType::AuthHookCaller)
        {
            return Err(RuntimeError::InvalidArgument {
                argument,
                reason: format!("Authorize hook '{}' must take an AuthHookCaller as argument 2", hook),
            });
        }

        Ok(())
    }

    /// Validates the shape of a spend-script predicate `FunctionDef`, mirroring `check_resource_auth_hook`. A spend
    /// script must be a unit-returning static function whose last argument is a `SpendContext`. Taking no component
    /// is load-bearing: a predicate that locked a component could write it, defeating the read-only guarantee.
    fn validate_spend_script_signature(func: &FunctionDef) -> Result<(), RuntimeError> {
        if func.kind != FunctionKind::Function {
            return Err(RuntimeError::InvalidArgument {
                argument: "TemplateFunction",
                reason: format!(
                    "spend script function '{}' must be a template function, not a method or a migration",
                    func.name
                ),
            });
        }
        if !matches!(func.output, Type::Unit) {
            return Err(RuntimeError::InvalidArgument {
                argument: "TemplateFunction",
                reason: format!(
                    "spend script function '{}' must return unit (it rejects by panicking, not by returning a value)",
                    func.name
                ),
            });
        }
        match func.arguments.last() {
            Some(arg) if arg.arg_type.is_well_known(WellKnownType::SpendContext) => Ok(()),
            _ => Err(RuntimeError::InvalidArgument {
                argument: "TemplateFunction",
                reason: format!(
                    "spend script function '{}' must take a SpendContext as its last argument",
                    func.name
                ),
            }),
        }
    }

    /// Resolves a `TemplateFunction` spend-condition leaf against its template and validates it end-to-end: the
    /// referenced function exists, has the required signature, and `args` carries exactly one element per leading
    /// (non-`SpendContext`) parameter. Each element is charged for and decoded once, when the script is invoked, and
    /// its type is checked by the dispatcher's `decode_exact::<T>`. Because a condition tree commits only an opaque
    /// root, leaves are hidden at creation; this runs at spend time (T2) when the leaf is revealed. Templates are
    /// immutable substates, so a referenced template must already resolve — there is no "not yet published" skip
    /// case.
    fn validate_template_function(&self, tf: &TemplateFunction) -> Result<(), RuntimeError> {
        let template_def = self.get_template_def(&tf.template)?;
        let func = template_def
            .get_function(&tf.function)
            .ok_or_else(|| RuntimeError::InvalidArgument {
                argument: "TemplateFunction",
                reason: format!(
                    "spend script function '{}' not found on template {}",
                    tf.function, tf.template
                ),
            })?;
        Self::validate_spend_script_signature(func)?;

        // `TemplateFunction.args` is positional: one CBOR value per leading parameter. Signature validation above
        // guarantees there is at least the trailing `SpendContext` argument, so this subtraction cannot underflow.
        let expected_bound_args = func.arguments.len() - 1;
        if tf.args.len() != expected_bound_args {
            return Err(RuntimeError::InvalidArgument {
                argument: "TemplateFunction",
                reason: format!(
                    "spend script '{}' expects {} bound argument(s) but {} were provided",
                    tf.function,
                    expected_bound_args,
                    tf.args.len()
                ),
            });
        }
        Ok(())
    }

    /// Authorises every spent input of a transfer before the spend executes, so that a rejection leaves the inputs
    /// unspent. This is the single, mandatory authorization gate for stealth spends; the execution path
    /// ([`WorkingState::validate_and_spend_stealth_utxos`]) performs no auth of its own.
    ///
    /// Each input's committed [`SpendAuthorization`] is read once, then the per-input [`SpendWitness`] selects the
    /// path:
    /// - **key path** — the output's `spend_key` must be present and its signer badge in the transaction's auth scope;
    /// - **script path** — the revealed leaf must be included under the output's committed `condition_root` (the
    ///   inclusion proof is verified exactly once, here), then the leaf is evaluated: an `AccessRule` leaf natively, a
    ///   `TemplateFunction` leaf as a read-only WASM predicate.
    fn verify_input_authorizations(
        &self,
        resource_address: ResourceAddressRef,
        statement: &StealthTransferStatement,
    ) -> Result<(), RuntimeError> {
        if statement.inputs_statement.inputs.is_empty() {
            return Ok(());
        }
        // Covenant predicates look claims up by partition, so the claim list is bounded before any predicate runs.
        stealth::validate_covenant_claims(statement)?;
        let resolved = self
            .tracker
            .read_with(|state| state.resolve_resource_address_ref(resource_address))?;

        // Read each input's committed authorisation once, parallel to the statement's inputs.
        let input_auths = self.tracker.write_with(|state| {
            statement
                .inputs_statement
                .inputs
                .iter()
                .map(|input| state.get_stealth_utxo_spend_auth(resolved, input))
                .collect::<Result<Vec<_>, RuntimeError>>()
        })?;

        // A covenant predicate partitions inputs by `condition_root`, so it needs the roots of all inputs, not just the
        // one it gates. Key-path inputs (which never join a covenant partition) contribute `None`.
        let input_condition_roots: Vec<_> = statement
            .inputs_statement
            .inputs
            .iter()
            .zip(&input_auths)
            .map(|(input, auth)| {
                if input.witness.is_key_path() {
                    None
                } else {
                    auth.condition_root().copied()
                }
            })
            .collect();
        let view = Rc::new(StatementSpendView::new(statement, input_condition_roots));

        for (index, input) in statement.inputs_statement.inputs.iter().enumerate() {
            match &input.witness {
                SpendWitness::KeyPath => {
                    let Some(pk) = input_auths[index].spend_key() else {
                        return Err(RuntimeError::ResourceError(ResourceError::InvalidSpend {
                            details: format!(
                                "Key-path witness provided for stealth UTXO {} which has no spend_key",
                                input.commitment
                            ),
                        }));
                    };
                    let badge = NonFungibleAddress::from_public_key(*pk);
                    let in_scope = self
                        .tracker
                        .read_with(|state| state.base_call_scope().auth_scope().contains_badge(&badge));
                    if !in_scope {
                        return Err(RuntimeError::ResourceError(
                            ResourceError::RequiredSignatureMissingForStealthUtxo {
                                commitment: input.commitment,
                                public_key: *pk,
                            },
                        ));
                    }
                },
                SpendWitness::ScriptPath { leaf, proof, data } => {
                    self.verify_script_path_authorization(leaf, proof, data, index, input, &view)?;
                },
            }
        }
        Ok(())
    }

    /// Verifies one script-path input authorisation: bounds the spender-supplied witness data and inclusion proof,
    /// validates the revealed leaf's structure, binds it to the committed `condition_root`, then evaluates it.
    fn verify_script_path_authorization(
        &self,
        leaf: &SpendCondition,
        proof: &MerkleProof,
        data: &Bytes,
        index: usize,
        input: &StealthInput,
        view: &Rc<StatementSpendView>,
    ) -> Result<(), RuntimeError> {
        // A `condition_root` is required to spend via the script path.
        let root = view.input_condition_roots[index].ok_or_else(|| {
            RuntimeError::ResourceError(ResourceError::InvalidSpend {
                details: format!(
                    "Script-path witness provided for stealth UTXO {} which has no condition_root",
                    input.commitment
                ),
            })
        })?;
        // Witness data is processed natively by the leaf's predicates, so bound its size.
        let max_witness_data_len = limits::STEALTH_LIMITS.max_witness_data_len;
        if data.len() > max_witness_data_len {
            return Err(RuntimeError::ResourceError(ResourceError::InvalidSpend {
                details: format!(
                    "Spend witness data for stealth UTXO {} is {} bytes, exceeding the limit of {max_witness_data_len}",
                    input.commitment,
                    data.len()
                ),
            }));
        }
        // The inclusion proof is spender-supplied and each sibling costs a native hash, so bound its length before
        // folding it in `verify_inclusion`.
        let max_inclusion_proof_len = limits::STEALTH_LIMITS.max_inclusion_proof_len;
        if proof.siblings.len() > max_inclusion_proof_len {
            return Err(RuntimeError::ResourceError(ResourceError::InvalidSpend {
                details: format!(
                    "Inclusion proof for stealth UTXO {} has {} siblings, exceeding the limit of \
                     {max_inclusion_proof_len}",
                    input.commitment,
                    proof.siblings.len()
                ),
            }));
        }
        // The revealed leaf is untrusted spender data: validate its structure before hashing, so an adversarial
        // nesting cannot exhaust the stack in the hasher or the evaluator.
        Self::validate_condition_structure(leaf, &input.commitment)?;
        // Bind the revealed leaf to the committed root before evaluating it, so the spender cannot substitute a leaf
        // that was never committed.
        if !stealth::verify_inclusion(stealth::condition_leaf_hash(leaf), proof, root) {
            return Err(RuntimeError::ResourceError(ResourceError::InvalidSpend {
                details: format!(
                    "Revealed spend condition leaf is not committed in the condition_root of stealth UTXO {}",
                    input.commitment
                ),
            }));
        }
        self.evaluate_condition_leaf(leaf, index as u32, input.commitment, root, view, data.as_slice())
    }

    /// Validates a revealed condition leaf's structure before it is hashed or evaluated. The rule itself lives in
    /// [`stealth::validate_condition_structure`] so the wallet can apply the same admissibility test before committing
    /// funds to a tree; this only attributes a failure to the UTXO being spent.
    fn validate_condition_structure(
        leaf: &SpendCondition,
        commitment: &PedersenCommitmentBytes,
    ) -> Result<(), RuntimeError> {
        stealth::validate_condition_structure(leaf).map_err(|err| {
            RuntimeError::ResourceError(ResourceError::InvalidSpend {
                details: format!("Spend condition for stealth UTXO {commitment}: {err}"),
            })
        })
    }

    /// Evaluates a revealed condition leaf that has already been proven included in the committed root. The leaf is a
    /// conjunction: every atom must hold (logical AND). The single witness `data` blob is shared by the whole leaf;
    /// each atom interprets it as it expects (a data-consuming builtin owns it entirely, which
    /// `validate_condition_structure` guarantees by rejecting any other consumer).
    fn evaluate_condition_leaf(
        &self,
        leaf: &SpendCondition,
        input_index: u32,
        input_commitment: PedersenCommitmentBytes,
        root: Hash32,
        view: &Rc<StatementSpendView>,
        data: &[u8],
    ) -> Result<(), RuntimeError> {
        for condition in leaf.conditions() {
            self.evaluate_atomic_condition(condition, input_index, input_commitment, root, view, data)?;
        }
        Ok(())
    }

    /// Evaluates a single [`AtomicCondition`] of a conjunction leaf: an access rule against the auth scope, a WASM
    /// [`TemplateFunction`], a native [`BuiltinPredicate`], or a native [`Covenant`].
    fn evaluate_atomic_condition(
        &self,
        condition: &AtomicCondition,
        input_index: u32,
        input_commitment: PedersenCommitmentBytes,
        root: Hash32,
        view: &Rc<StatementSpendView>,
        data: &[u8],
    ) -> Result<(), RuntimeError> {
        match condition {
            AtomicCondition::AccessRule(access_rule) => {
                // The engine rejects an invalid access rule (an empty `any_of`/`all_of`, or an `m_of_n` with an
                // out-of-range threshold or a duplicate requirement) wherever it stores a rule. A stealth output
                // commits only to a hash of its spend conditions, so its rules are first seen here and must fail
                // closed.
                reject_invalid_rule("spend_condition", access_rule.find_invalid_rule())?;
                let allowed = self
                    .tracker
                    .write_with_authorization(|state| state.authorization().check_access_rule(access_rule))?;
                if !allowed {
                    return Err(RuntimeError::AccessDenied {
                        action_ident: ActionIdent::Native(NativeAction::StealthUtxoSpend),
                    });
                }
            },
            AtomicCondition::TemplateFunction(tf) => {
                self.evaluate_spend_script(tf, input_index, input_commitment, root, view, data)?;
            },
            AtomicCondition::Builtin(predicate) => {
                self.evaluate_builtin(predicate, data)?;
            },
            AtomicCondition::Covenant(covenant) => {
                self.evaluate_covenant(covenant, root, view)?;
            },
        }
        Ok(())
    }

    /// Evaluates a single native [`BuiltinPredicate`] — a local spend predicate (timelock or hashlock) — rejecting the
    /// spend with [`RuntimeError::SpendConditionNotMet`] if it does not hold. A data-consuming predicate (the hashlock)
    /// reads the entire witness `data` blob as raw bytes; `validate_condition_structure` guarantees it is the leaf's
    /// sole consumer, so the whole blob is unambiguously its input.
    fn evaluate_builtin(&self, predicate: &BuiltinPredicate, data: &[u8]) -> Result<(), RuntimeError> {
        let satisfied = match predicate {
            BuiltinPredicate::AfterEpoch(unlock_epoch) => self.tracker.get_current_epoch()?.as_u64() >= *unlock_epoch,
            BuiltinPredicate::BeforeEpoch(deadline_epoch) => {
                self.tracker.get_current_epoch()?.as_u64() < *deadline_epoch
            },
            BuiltinPredicate::HashLock { hash, alg } => stealth::hashlock_digest(*alg, data) == *hash,
        };
        if !satisfied {
            return Err(RuntimeError::SpendConditionNotMet {
                details: format!("builtin predicate not satisfied: {predicate:?}"),
            });
        }
        Ok(())
    }

    /// Evaluates a single native [`Covenant`] over the spending transfer, rejecting the spend with
    /// [`RuntimeError::SpendConditionNotMet`] if it does not hold. `root` is the committed `condition_root` of the
    /// input being spent, which keys its covenant partition.
    fn evaluate_covenant(
        &self,
        covenant: &Covenant,
        root: Hash32,
        view: &StatementSpendView,
    ) -> Result<(), RuntimeError> {
        let satisfied = match covenant {
            Covenant::OutputPreservesCondition => view.output_preserves_condition(&root),
            Covenant::OutputTo {
                condition_root,
                min_value,
            } => view.has_output_to(condition_root, *min_value),
            Covenant::BalancePreserved(max_revealed) => view.covenant_balanced(&root, *max_revealed, |points| {
                self.tracker.charge_native_execution(points)
            })?,
        };
        if !satisfied {
            return Err(RuntimeError::SpendConditionNotMet {
                details: format!("covenant not satisfied: {covenant:?}"),
            });
        }
        Ok(())
    }

    /// Invokes a single `TemplateFunction` spend-condition predicate inside a read-only restricted frame. Returning
    /// normally authorises the spend; any panic — a deliberate `assert!`, out-of-gas, or a blocked state mutation
    /// (`WriteInReadOnlyContext`) — aborts it as `SpendScriptRejected`.
    fn evaluate_spend_script(
        &self,
        tf: &TemplateFunction,
        input_index: u32,
        input_commitment: PedersenCommitmentBytes,
        current_input_condition_root: Hash32,
        view: &Rc<StatementSpendView>,
        witness_data: &[u8],
    ) -> Result<(), RuntimeError> {
        self.invoke_modules_on_runtime_call("evaluate_spend_script")?;
        // (T2) Authoritative spend-time validation. A revealed leaf is untrusted spender data, so the function shape
        // and bound-arg count are validated immediately before invoking.
        self.validate_template_function(tf)?;

        let exec = SpendScriptExecution {
            statement: view.clone(),
            current_input_index: input_index,
            current_input_commitment: input_commitment,
            current_input_condition_root,
            witness_data: witness_data.to_vec(),
        };

        // Assemble the call args as [bound args..., injected SpendContext handle], exactly as the auth hook appends
        // its injected `auth_caller`. The generated dispatcher decodes each slot positionally with `decode_exact::<T>`.
        let mut args = Vec::with_capacity(tf.args.len() + 1);
        args.extend(tf.args.iter().cloned().map(InstructionArg::Literal));
        args.push(InstructionArg::Literal(
            tari_bor::encode(&SpendContext::new(input_index))?.into(),
        ));

        // Make the introspection context reachable for the duration of the call, and restrict the predicate's frame
        // to a read-only, non-cross-template sandbox.
        let prev_exec_context = self.spend_exec_context.replace(Some(exec));
        let result = self.invoke_template_function(&tf.template, &tf.function, args, Some(FrameWriteMode::ReadOnly));
        *self.spend_exec_context.borrow_mut() = prev_exec_context;

        result
            .map(|_| ())
            .map_err(|e| RuntimeError::SpendScriptRejected { details: Box::new(e) })
    }

    /// Charges for `ids` non-fungible ids an engine call moved from one container to another. The
    /// count is read off the container that moved, so the charge follows the move it pays for, and
    /// a transaction that cannot cover it fails out of compute having made that one move.
    fn charge_non_fungible_ids_moved(&self, ids: usize) -> Result<(), RuntimeError> {
        self.tracker
            .charge_native_execution(tari_engine_types::limits::non_fungible_move_points(ids as u64))
    }

    /// Charges for `ids` non-fungible ids an engine call locked into a new proof. They move twice:
    /// into the proof now, and back when the proof is dropped, explicitly or with its frame. Both
    /// moves are charged here, since a frame's proofs can be dropped where no engine call is made.
    fn charge_non_fungible_ids_locked(&self, ids: usize) -> Result<(), RuntimeError> {
        self.charge_non_fungible_ids_moved(ids.saturating_mul(2))
    }

    /// Charges [`Self::charge_engine_arg`] for an argument's CBOR, then decodes it into the `Value`
    /// the call is invoked with.
    ///
    /// Every argument the engine decodes for a call goes through here: the decode, the indexing of the
    /// value for well-known types and its encoding into the callee's `CallInfo` all run outside the
    /// Wasmer meter, and transaction weight prices an argument's bytes, which follow none of that.
    fn decode_charged_arg(&self, bytes: &[u8]) -> Result<tari_bor::Value, RuntimeError> {
        self.charge_engine_arg(bytes).map_err(|e| match e {
            EngineArgChargeError::Unreadable(e) => RuntimeError::from(e),
            EngineArgChargeError::Runtime(e) => e,
        })?;
        Ok(decode_exact(bytes)?)
    }

    /// Charges `points(input.len(), 0)` for the bytes of the CBOR in `input`, then counts its items
    /// and charges `points(0, items)` for them.
    ///
    /// Counting the items is itself host work, so the walk reads only as many items as the
    /// transaction can still pay `per_item` for, and checks their nesting against `max_depth` on the
    /// way. Input holding more is charged what was read and fails out of compute. Input the walk
    /// rejects, malformed or nested too deep, is charged the items read before the walk stopped.
    fn charge_cbor_read(
        &self,
        input: &[u8],
        max_depth: usize,
        per_item: u64,
        points: fn(u64, u64) -> u64,
    ) -> Result<(), EngineArgChargeError> {
        self.tracker.charge_native_execution(points(input.len() as u64, 0))?;
        let affordable = self.tracker.native_execution_headroom() / per_item;
        let counted = match tari_bor::count_data_items_bounded(input, max_depth, affordable) {
            Ok(counted) => counted,
            Err(tari_bor::ItemCountError { items_read, error }) => {
                self.tracker.charge_native_execution(points(0, items_read))?;
                return Err(EngineArgChargeError::Unreadable(error.into()));
            },
        };
        match counted {
            Some(items) => self.tracker.charge_native_execution(points(0, items))?,
            None => {
                // The items read are paid for before failing, which leaves nothing for a repeat of
                // the call to read.
                self.tracker.charge_native_execution(points(0, affordable))?;
                self.tracker.charge_native_execution(points(0, 1))?;
                return Err(RuntimeError::InvariantError {
                    function: "charge_cbor_read",
                    details: "an item beyond the native execution headroom was affordable".to_string(),
                }
                .into());
            },
        }
        Ok(())
    }

    /// Charges [`tari_engine_types::limits::component_load_points`] for what `address`'s state holds,
    /// counting no more of its items than the native execution headroom can pay for.
    fn charge_component_load(&self, address: ComponentAddress) -> Result<(), RuntimeError> {
        use tari_engine_types::limits::{NativeExecutionPoints, component_load_points};

        let affordable = self.tracker.native_execution_headroom() / NativeExecutionPoints::PER_COMPONENT_STATE_ITEM;
        let counted = self.tracker.write_with(|state_mut| {
            let component = state_mut.load_and_cache_component(address)?;
            Ok::<_, RuntimeError>(
                component
                    .state()
                    .count_items_bounded(affordable, IndexedWellKnownTypes::indexes_tag),
            )
        })?;
        match counted {
            // The items, which pay for the count, are charged first, so a state whose references are
            // unaffordable fails having paid for what was counted.
            Some(count) => {
                self.tracker
                    .charge_native_execution(component_load_points(count.items, 0))?;
                self.tracker
                    .charge_native_execution(component_load_points(0, count.counted_tags))
            },
            None => {
                // The items counted are paid for before failing, which leaves nothing for a repeat of
                // the load to count.
                self.tracker
                    .charge_native_execution(component_load_points(affordable, 0))?;
                self.tracker.charge_native_execution(component_load_points(1, 0))?;
                Err(RuntimeError::InvariantError {
                    function: "charge_component_load",
                    details: "an item beyond the native execution headroom was affordable".to_string(),
                })
            },
        }
    }
}

impl<TStore, TTemplateProvider> RuntimeInterface for RuntimeInterfaceImpl<TStore, TTemplateProvider>
where
    TStore: StateReader + Clone + 'static,
    TTemplateProvider: TemplateProvider<Template = LoadedTemplate>,
{
    fn next_entity_id(&self) -> Result<EntityId, RuntimeError> {
        let id = self.entity_id_provider.borrow_mut().next_entity_id()?;
        Ok(id)
    }

    fn emit_event(&self, topic: String, payload: Metadata) -> Result<(), RuntimeError> {
        if let Err(reason) = Event::validate_custom_topic(&topic) {
            return Err(RuntimeError::InvalidEventTopic { topic, reason });
        }
        // The template name is guest-supplied and, alone or joined to a dotted topic, can spell a
        // reserved topic, so the prefixed topic is validated as a whole.
        let module = self.tracker.get_template_module_name()?;
        let topic = format!("{module}.{topic}");
        if let Err(reason) = Event::validate_custom_topic(&topic) {
            return Err(RuntimeError::InvalidEventTopic { topic, reason });
        }
        Self::check_metadata_values("payload", &payload)?;

        self.invoke_modules_on_runtime_call("emit_event")?;

        let component_address_option = self.tracker.read_with(|state| {
            Ok::<_, RuntimeError>(
                state
                    .current_call_scope()?
                    .get_current_component_lock()
                    .and_then(|l| l.substate_id().as_component_address()),
            )
        })?;
        let substate_id = component_address_option.map(SubstateId::Component);
        let template_address = self.tracker.get_template_address()?;

        self.tracker
            .add_event(Event::custom(substate_id, template_address, topic, payload))?;
        Ok(())
    }

    fn emit_log(&self, level: LogLevel, message: String) -> Result<(), RuntimeError> {
        self.invoke_modules_on_runtime_call("emit_log")?;

        let log_level = match level {
            LogLevel::Error => log::Level::Error,
            LogLevel::Warn => log::Level::Warn,
            LogLevel::Info => log::Level::Info,
            LogLevel::Debug => log::Level::Debug,
        };

        let size_bytes = message.len();
        // Echoed to the validator's log and charged after the entry is accepted, so the bytes
        // written and billed for are the bytes retained: the size and count limits `add_log`
        // enforces are what decides that.
        self.tracker.add_log(LogEntry::new(level, message), |entry| {
            log::log!(target: "tari::ootle::engine::runtime", log_level, "{}", entry.message);
        })?;
        self.invoke_modules_on_runtime_event(RuntimeEvent::LogEmitted { size_bytes })?;
        Ok(())
    }

    fn load_component(&self, call: ComponentReference) -> Result<LoadedComponent, RuntimeError> {
        self.invoke_modules_on_runtime_call("load_component")?;
        let address = match call {
            ComponentReference::Address(address) => address,
            ComponentReference::Workspace(id) => self.tracker.write_with(|state_mut| {
                let id = WorkspaceOffsetId::new(id);
                let value = state_mut
                    .workspace()
                    .get(id)?
                    .ok_or_else(|| RuntimeError::ItemNotOnWorkspace {
                        id,
                        existing_ids: state_mut.workspace().all_ids_iter().collect(),
                    })?;

                // If the value is a ComponentAddress, use it directly. If it's an integer, treat it as an allocation
                // ID.
                let address = if value.is_tag_of::<ComponentAddress>() {
                    tari_bor::from_value(value).map_err(|e| RuntimeError::InvalidArgument {
                        argument: "ComponentCall::FromWorkspace",
                        reason: format!("Item on workspace at key '{id}' is not a valid ComponentAddress: {e}",),
                    })?
                } else if value.is_tag_of::<ComponentAddressAllocation>() {
                    let allocation_id: ComponentAddressAllocation =
                        tari_bor::from_value(value).map_err(|e| RuntimeError::InvalidArgument {
                            argument: "ComponentCall::FromWorkspace",
                            reason: format!(
                                "Item on workspace at key '{id}' is not a valid ComponentAddressAllocation: {e}",
                            ),
                        })?;
                    let substate_id = state_mut.get_substate_id_from_used_address_allocation(allocation_id.id())?;
                    match substate_id {
                        SubstateId::Component(addr) => addr,
                        substate_id => {
                            let substate_type =
                                tari_ootle_common_types::substate_type::SubstateType::from(&substate_id);
                            return Err(RuntimeError::InvalidArgument {
                                argument: "ComponentCall::Allocation",
                                reason: format!(
                                    "Invalid attempt to load component with an address allocation ID ({}) with \
                                     substate type {substate_type}",
                                    allocation_id.id()
                                ),
                            });
                        },
                    }
                } else {
                    return Err(RuntimeError::InvalidArgument {
                        argument: "ComponentCall::FromWorkspace",
                        reason: format!(
                            "Item on workspace at key '{id}' is not a valid ComponentAddress or \
                             ComponentAddressAllocation",
                        ),
                    });
                };
                Ok::<_, RuntimeError>(address)
            })?,
        };
        self.charge_component_load(address)?;
        self.tracker.write_with(|state_mut| {
            let component = state_mut.load_and_cache_component(address)?;
            Ok(LoadedComponent {
                address,
                template_address: *component.template_address(),
                entity_id: *component.entity_id(),
                state_scope: IndexedWellKnownTypes::from_value(component.state())?,
            })
        })
    }

    fn lock_component(&self, address: ComponentAddress, lock_flag: LockFlag) -> Result<LockedSubstate, RuntimeError> {
        self.tracker.lock_substate(SubstateId::Component(address), lock_flag)
    }

    #[allow(clippy::too_many_lines)]
    fn component_invoke(
        &self,
        component_ref: ComponentRef,
        action: ComponentAction,
        args: EngineArgs,
    ) -> Result<InvokeResult, RuntimeError> {
        self.invoke_modules_on_runtime_call("component_invoke")?;

        trace!(
            target: LOG_TARGET,
            "Component invoke: {} {:?}",
            component_ref,
            action,
        );

        match action {
            ComponentAction::Create => {
                let CreateComponentArg {
                    encoded_state,
                    owner_rule,
                    access_rules,
                    address_allocation,
                    upgrade_rule,
                } = args.assert_one_arg()?;

                let template_addr = self.tracker.get_template_address()?;
                let template_def = self.get_template_def(&template_addr)?;
                validate_component_access_rule_methods(&access_rules, &template_def)?;

                if access_rules.contains_scoped_to_component_or_template() {
                    return Err(RuntimeError::InvalidArgument {
                        argument: "access_rules",
                        reason: "component(..)/template(..) cannot be used on a component method access rule"
                            .to_string(),
                    });
                }
                reject_invalid_rule("access_rules", access_rules.find_invalid_rule())?;
                reject_invalid_rule("owner_rule", find_invalid_owner_rule(&owner_rule))?;
                // A component owner rule is only ever evaluated with the component's own frame on top, so
                // `component(..)`/`template(..)` would be constant (true for the component's own address).
                if let OwnerRule::ByAccessRule(rule) = &owner_rule &&
                    rule.contains_scoped_to_component_or_template()
                {
                    return Err(RuntimeError::InvalidArgument {
                        argument: "owner_rule",
                        reason: "component(..)/template(..) cannot be used in a component owner rule".to_string(),
                    });
                }
                // An upgrade rule is evaluated with the migration frame on top. There `component(..)` is constant (the
                // component being upgraded) and `template(..)` matches the upgrade target, which the upgrader picks, so
                // neither expresses who may upgrade.
                if let UpdateRule::AccessRule(rule) = &upgrade_rule {
                    if rule.contains_scoped_to_component_or_template() {
                        return Err(RuntimeError::InvalidArgument {
                            argument: "upgrade_rule",
                            reason: "component(..)/template(..) cannot be used in a component upgrade rule".to_string(),
                        });
                    }
                    reject_invalid_rule("upgrade_rule", rule.find_invalid_rule())?;
                }

                let owner_rule = match owner_rule {
                    OwnerRule::OwnedBySigner => {
                        SubstateOwnerRule::ByPublicKey(self.main_signer("OwnerRule::OwnedBySigner")?)
                    },
                    OwnerRule::None => SubstateOwnerRule::None,
                    OwnerRule::ByAccessRule(rule) => SubstateOwnerRule::ByAccessRule(rule),
                    OwnerRule::ByPublicKey(key) => SubstateOwnerRule::ByPublicKey(key),
                };

                let component_address = self.tracker.new_component(
                    encoded_state,
                    owner_rule,
                    access_rules,
                    address_allocation,
                    upgrade_rule,
                )?;
                Ok(InvokeResult::encode(&component_address)?)
            },
            ComponentAction::GetState => {
                let component_address =
                    component_ref
                        .as_component_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "component_ref",
                            reason: "GetState component action requires a component address".to_string(),
                        })?;
                args.assert_no_args("ComponentAction::GetState")?;
                let is_already_locked = self.tracker.read_with(|state| {
                    let is_already_locked = state
                        .current_call_scope()?
                        .get_current_component_lock()
                        .is_some_and(|l| *l.substate_id() == component_address);
                    // Checked before charging the load so an out-of-scope read costs nothing
                    if !is_already_locked {
                        state.check_substate_in_scope(&SubstateId::Component(component_address))?;
                    }
                    Ok::<_, RuntimeError>(is_already_locked)
                })?;

                if !is_already_locked {
                    self.charge_component_load(component_address)?;
                }

                self.tracker.write_with(|state| {
                    let component_lock = if is_already_locked {
                        state
                            .current_call_scope()?
                            .get_current_component_lock()
                            .cloned()
                            .ok_or(RuntimeError::NotInComponentContext {
                                action: ComponentAction::GetState.into(),
                            })?
                    } else {
                        state.read_lock_substate(SubstateId::Component(component_address))?
                    };

                    // We only allow mutating of the current component.
                    if *component_lock.substate_id() != component_address {
                        return Err(RuntimeError::LockError(LockError::SubstateNotLocked {
                            address: SubstateId::Component(component_address),
                        }));
                    }

                    let component = state.get_component(&component_lock)?;
                    let result = InvokeResult::encode(component.state())?;
                    if !is_already_locked {
                        state.unlock_substate(component_lock)?;
                    }

                    Ok(result)
                })
            },
            ComponentAction::SetState => {
                let component_address =
                    component_ref
                        .as_component_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "component_ref",
                            reason: "SetState component action should not define a specific component address"
                                .to_string(),
                        })?;
                let component_state = args.assert_one_arg()?;
                self.tracker.write_with(|state| {
                    let component_lock = state
                        .current_call_scope()?
                        .get_current_component_lock()
                        .cloned()
                        .ok_or(RuntimeError::NotInComponentContext {
                            action: ComponentAction::SetState.into(),
                        })?;

                    // We only allow mutating of the current component. Note this check doesnt actually provide any
                    // security itself, it's just checking the engine call is made correctly. The security comes from
                    // the fact that the engine creates the lock on the currently executing component and that is the
                    // lock we use to gain access.
                    if *component_lock.substate_id() != component_address {
                        return Err(RuntimeError::AccessDeniedSetComponentState {
                            attempted_on: component_address.into(),
                            attempted_by: Box::new(component_lock.substate_id().clone()),
                        });
                    }

                    state.modify_component_with(&component_lock, |component| {
                        if component_state == *component.state() {
                            return false;
                        }
                        component.body.set(component_state);
                        true
                    })?;

                    Ok(InvokeResult::unit())
                })
            },
            ComponentAction::SetAccessRules => {
                let component_address =
                    component_ref
                        .as_component_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "component_ref",
                            reason: "SetAccessRules component action requires a component address".to_string(),
                        })?;

                let access_rules: ComponentAccessRules = args.assert_one_arg()?;

                let template_addr = self.tracker.get_template_address()?;
                let template_def = self.get_template_def(&template_addr)?;
                validate_component_access_rule_methods(&access_rules, &template_def)?;

                if access_rules.contains_scoped_to_component_or_template() {
                    return Err(RuntimeError::InvalidArgument {
                        argument: "access_rules",
                        reason: "component(..)/template(..) cannot be used on a component method access rule"
                            .to_string(),
                    });
                }
                reject_invalid_rule("access_rules", access_rules.find_invalid_rule())?;

                self.tracker.write_with_authorization(|state| {
                    let component_lock = state
                        .current_call_scope()?
                        .get_current_component_lock()
                        .cloned()
                        .ok_or(RuntimeError::NotInComponentContext {
                            action: ComponentAction::SetAccessRules.into(),
                        })?;
                    // We only allow mutating of the current component. Note this check doesnt actually provide any
                    // security itself, it's just checking the engine call is made correctly. The security comes from
                    // the fact that the engine creates the lock on the currently executing component and that is the
                    // lock we use to gain access.
                    if *component_lock.substate_id() != component_address {
                        return Err(RuntimeError::LockError(LockError::SubstateNotLocked {
                            address: SubstateId::Component(component_address),
                        }));
                    }
                    let component = state.get_component(&component_lock)?;
                    state
                        .authorization()
                        .require_ownership(ComponentAction::SetAccessRules, component.as_ownership())?;

                    if access_rules != *component.access_rules() {
                        state.update_component_header(&component_lock, |header| header.access_rules = access_rules)?;
                    }

                    Ok::<_, RuntimeError>(())
                })?;

                Ok(InvokeResult::unit())
            },
            ComponentAction::SetOwnerRule => {
                let component_address =
                    component_ref
                        .as_component_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "component_ref",
                            reason: "SetOwnerRule component action requires a component address".to_string(),
                        })?;

                let owner_rule: SubstateOwnerRule = args.assert_one_arg()?;

                // A component owner rule is only ever evaluated with the component's own frame on top, so
                // `component(..)`/`template(..)` would be constant (true for the component's own address).
                if let SubstateOwnerRule::ByAccessRule(rule) = &owner_rule &&
                    rule.contains_scoped_to_component_or_template()
                {
                    return Err(RuntimeError::InvalidArgument {
                        argument: "owner_rule",
                        reason: "component(..)/template(..) cannot be used in a component owner rule".to_string(),
                    });
                }
                if let SubstateOwnerRule::ByAccessRule(rule) = &owner_rule {
                    reject_invalid_rule("owner_rule", rule.find_invalid_rule())?;
                }

                self.tracker.write_with_authorization(|state| {
                    let component_lock = state
                        .current_call_scope()?
                        .get_current_component_lock()
                        .cloned()
                        .ok_or(RuntimeError::NotInComponentContext {
                            action: ComponentAction::SetOwnerRule.into(),
                        })?;
                    // Only the current component may be mutated. The component lock is created by the engine for the
                    // executing component, so this only checks that the engine call names it.
                    if *component_lock.substate_id() != component_address {
                        return Err(RuntimeError::LockError(LockError::SubstateNotLocked {
                            address: SubstateId::Component(component_address),
                        }));
                    }
                    let component = state.get_component(&component_lock)?;
                    // Only the current owner rule gates this; the new rule may name anyone, which is how ownership
                    // is handed over. `SubstateOwnerRule::None` is satisfied by no caller, so setting it is final.
                    state
                        .authorization()
                        .require_ownership(ComponentAction::SetOwnerRule, component.as_ownership())?;

                    if owner_rule != *component.owner_rule() {
                        state.update_component_header(&component_lock, |header| header.owner_rule = owner_rule)?;
                    }

                    Ok::<_, RuntimeError>(())
                })?;

                Ok(InvokeResult::unit())
            },
            ComponentAction::GetTemplateAddress => {
                let component_address =
                    component_ref
                        .as_component_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "component_ref",
                            reason: "GetTemplateAddress component action requires a component address".to_string(),
                        })?;

                args.assert_no_args("Component::GetTemplateAddress")?;

                self.tracker.write_with(|state| {
                    let current_component_lock = state
                        .current_call_scope()?
                        .get_current_component_lock()
                        .filter(|l| *l.substate_id() == component_address)
                        .cloned();
                    let is_already_locked = current_component_lock.is_some();
                    let locked = match current_component_lock {
                        Some(lock) => lock,
                        None => state.read_lock_substate_in_scope(SubstateId::Component(component_address))?,
                    };
                    let component = state.get_component(&locked)?;
                    let template_address = *component.template_address();
                    if !is_already_locked {
                        state.unlock_substate(locked)?;
                    }
                    Ok(InvokeResult::encode(&template_address)?)
                })
            },
        }
    }

    #[allow(clippy::too_many_lines)]
    fn resource_invoke(
        &self,
        resource_ref: ResourceRef,
        action: ResourceAction,
        args: EngineArgs,
    ) -> Result<InvokeResult, RuntimeError> {
        self.invoke_modules_on_runtime_call("resource_invoke")?;

        debug!(
            target: LOG_TARGET,
            "Resource invoke: {} {:?}",
            resource_ref,
            action,
        );

        match action {
            ResourceAction::Create => {
                let arg: CreateResourceArg = args.assert_one_arg()?;

                if arg
                    .mint_arg
                    .as_ref()
                    .map(|mint| mint.as_resource_type() != arg.resource_type)
                    .unwrap_or(false)
                {
                    return Err(RuntimeError::InvalidArgument {
                        argument: "CreateResourceArg",
                        reason: "Mint argument type does not match resource type".to_string(),
                    });
                }

                if arg.view_key.is_some() && !arg.resource_type.is_confidential() && !arg.resource_type.is_stealth() {
                    return Err(RuntimeError::InvalidArgument {
                        argument: "CreateResourceArg",
                        reason: "View key can only be set for stealth or confidential resources".to_string(),
                    });
                }

                if let Some(ref view_key) = arg.view_key {
                    // Ristretto encodings are unique, so all-zero bytes is the only encoding of the identity.
                    if view_key.is_zero() {
                        return Err(RuntimeError::InvalidArgument {
                            argument: "CreateResourceArg",
                            reason: "View key cannot be the identity point".to_string(),
                        });
                    }
                    RistrettoPublicKey::from_canonical_bytes(view_key.as_bytes()).map_err(|e| {
                        RuntimeError::InvalidArgument {
                            argument: "CreateResourceArg",
                            reason: format!("View key is not a canonical Ristretto point: {e}"),
                        }
                    })?;
                }

                if arg.divisibility > limits::MAX_DIVISIBILITY {
                    return Err(RuntimeError::InvalidArgument {
                        argument: "CreateResourceArg",
                        reason: format!("Divisibility must be between 0 and {}", limits::MAX_DIVISIBILITY),
                    });
                }

                if arg.resource_type.is_non_fungible() && arg.divisibility > 0 {
                    return Err(RuntimeError::InvalidArgument {
                        argument: "CreateResourceArg",
                        reason: "Non-fungible resources cannot have divisibility".to_string(),
                    });
                }

                Self::check_metadata_values("metadata", &arg.metadata)?;
                Self::check_token_symbol(&arg.metadata)?;
                reject_invalid_rule("resource_access_rules", arg.access_rules.find_invalid_rule())?;
                reject_invalid_rule("resource_owner_rule", find_invalid_owner_rule(&arg.owner_rule))?;

                let owner_rule = match arg.owner_rule {
                    OwnerRule::OwnedBySigner => {
                        SubstateOwnerRule::ByPublicKey(self.main_signer("OwnerRule::OwnedBySigner")?)
                    },
                    OwnerRule::ByPublicKey(key) => SubstateOwnerRule::ByPublicKey(key),
                    OwnerRule::None => SubstateOwnerRule::None,
                    OwnerRule::ByAccessRule(rule) => SubstateOwnerRule::ByAccessRule(rule),
                };

                // Check that auth hook is valid
                if let Some(hook) = arg.authorize_hook.as_ref() {
                    self.check_resource_auth_hook("CreateResourceArg", hook)?;
                }

                // Charge the initial mint's native verification cost against the payment-funded
                // allowance before its proof crypto runs.
                if let Some(mint_arg) = arg.mint_arg.as_ref() {
                    self.charge_confidential_mint(
                        mint_arg,
                        arg.view_key.is_some(),
                        arg.is_total_supply_tracking_enabled,
                    )?;
                }

                self.tracker.write_with(|state_mut| {
                    let resource = Resource::new(
                        arg.resource_type,
                        owner_rule,
                        arg.access_rules,
                        arg.metadata,
                        arg.view_key,
                        arg.authorize_hook,
                        arg.divisibility,
                        arg.is_total_supply_tracking_enabled,
                    );

                    let resource_address = match arg.address_allocation {
                        Some(allocation) => {
                            let alloc = state_mut.use_allocated_address(allocation.id())?;
                            alloc.substate_id().as_resource_address().ok_or_else(|| {
                                RuntimeError::AddressAllocationTypeMismatch {
                                    id: alloc.substate_id().clone(),
                                    expected: "ResourceAddress",
                                }
                            })?
                        },
                        None => state_mut.id_provider()?.new_resource_address()?,
                    };

                    // The system's resource addresses must stay under the system's control: the genesis resources
                    // are created once, and the two caller-badge resources must stay empty for the engine's badges
                    // to be unforgeable.
                    if resource_address.is_system_reserved() {
                        return Err(RuntimeError::InvalidArgument {
                            argument: "resource_address",
                            reason: format!("Resource address {resource_address} is reserved by the system"),
                        });
                    }

                    Self::emit_std_event("resource", "create", resource_address, Metadata::new(), state_mut)?;

                    state_mut.new_substate(resource_address, resource)?;
                    let resource_lock = state_mut.write_lock_substate(SubstateId::Resource(resource_address))?;

                    let mut output_bucket = None;
                    if let Some(mint_arg) = arg.mint_arg {
                        let container = state_mut.mint_resource(&resource_lock, mint_arg)?;
                        let bucket_id = state_mut.id_provider()?.new_bucket_id()?;
                        state_mut.new_bucket(bucket_id, container)?;
                        output_bucket = Some(tari_template_lib::models::Bucket::from_id(bucket_id));
                    }

                    state_mut.unlock_substate(resource_lock)?;

                    Ok(InvokeResult::encode(&(resource_address, output_bucket))?)
                })
            },

            ResourceAction::GetTotalSupply => {
                let resource_address =
                    resource_ref
                        .as_resource_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "GetResourceType resource action requires a resource address".to_string(),
                        })?;
                args.assert_no_args("ResourceAction::GetTotalSupply")?;
                self.tracker.write_with(|state| {
                    let locked = state.read_lock_substate(SubstateId::Resource(resource_address))?;
                    let resource = state.get_resource(&locked)?;
                    let total_supply = resource.total_supply();
                    state.unlock_substate(locked)?;
                    Ok(InvokeResult::encode(&total_supply)?)
                })
            },
            ResourceAction::GetResourceInfo => {
                let resource_address =
                    resource_ref
                        .as_resource_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "GetResourceType resource action requires a resource address".to_string(),
                        })?;

                args.assert_no_args("ResourceAction::GetResourceType")?;

                self.tracker.write_with(|state| {
                    let locked = state.read_lock_substate(SubstateId::Resource(resource_address))?;
                    let resource = state.get_resource(&locked)?;
                    let resource_type = resource.resource_type();
                    let divisibility = resource.divisibility();
                    state.unlock_substate(locked)?;
                    Ok(InvokeResult::encode(&ResourceInfo {
                        resource_type,
                        divisibility,
                    })?)
                })
            },
            ResourceAction::Mint => {
                let resource_address =
                    resource_ref
                        .as_resource_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "Mint resource action requires a resource address".to_string(),
                        })?;
                let mint_resource: MintResourceArg = args.assert_one_arg()?;

                let (maybe_auth_hook, auth_caller, has_view_key, tracks_supply) =
                    self.tracker.write_with_authorization(|state_mut| {
                        let resource_lock = state_mut.read_lock_substate(SubstateId::Resource(resource_address))?;

                        let resource = state_mut.get_resource(&resource_lock)?;

                        state_mut.authorization().check_resource_access_rules(
                            ResourceAuthAction::Mint,
                            resource.as_ownership(),
                            resource.access_rules(),
                        )?;

                        let auth_caller = state_mut.get_auth_caller(&resource_lock)?;
                        let has_view_key = resource.view_key().is_some();
                        let tracks_supply = resource.is_supply_tracking_enabled();
                        let auth_hook = resource.auth_hook().cloned();
                        state_mut.unlock_substate(resource_lock)?;
                        Ok::<_, RuntimeError>((auth_hook, auth_caller, has_view_key, tracks_supply))
                    })?;

                if let Some(auth_hook) = maybe_auth_hook {
                    self.invoke_resource_access_hook(auth_hook, auth_caller, ResourceAuthAction::Mint)?;
                }

                let resource_lock = self.relock_resource(
                    resource_address,
                    if tracks_supply { LockFlag::Write } else { LockFlag::Read },
                )?;

                // Charge the mint's native verification cost against the payment-funded allowance
                // before its proof crypto runs.
                self.charge_confidential_mint(&mint_resource.mint_arg, has_view_key, tracks_supply)?;

                self.tracker.write_with(|state_mut| {
                    let mint_arg = mint_resource.mint_arg;

                    let resource = state_mut.mint_resource(&resource_lock, mint_arg)?;
                    let bucket_id = state_mut.id_provider()?.new_bucket_id()?;

                    let payload = try_metadata!("amount" => resource.unlocked_amount())?;
                    Self::emit_std_event("resource", "mint", resource_address, payload, state_mut)?;

                    state_mut.new_bucket(bucket_id, resource)?;
                    let bucket = tari_template_lib::models::Bucket::from_id(bucket_id);

                    state_mut.unlock_substate(resource_lock)?;

                    Ok(InvokeResult::encode(&bucket)?)
                })
            },
            ResourceAction::Recall => {
                let resource_address =
                    resource_ref
                        .as_resource_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "Recall resource action requires a resource address".to_string(),
                        })?;
                let arg: RecallResourceArg = args.assert_one_arg()?;

                let (maybe_auth_hook, auth_caller) = self.tracker.write_with_authorization(|state_mut| {
                    let resource_lock = state_mut.read_lock_substate(SubstateId::Resource(resource_address))?;

                    let resource = state_mut.get_resource(&resource_lock)?;
                    state_mut.authorization().check_resource_access_rules(
                        ResourceAuthAction::Recall,
                        resource.as_ownership(),
                        resource.access_rules(),
                    )?;

                    let auth_hook = resource.auth_hook().cloned();
                    let auth_caller = state_mut.get_auth_caller(&resource_lock)?;

                    state_mut.unlock_substate(resource_lock)?;
                    Ok::<_, RuntimeError>((auth_hook, auth_caller))
                })?;

                if let Some(auth_hook) = maybe_auth_hook {
                    self.invoke_resource_access_hook(auth_hook, auth_caller, ResourceAuthAction::Recall)?;
                }

                let (result, moved) = self.tracker.write_with(|state_mut| {
                    let vault_lock = state_mut.write_lock_substate(arg.vault_id.into())?;

                    // The recall rule that authorized this action belongs to `resource_address`, so it may only
                    // reach vaults holding that resource.
                    let vault_resource = *state_mut.get_vault(&vault_lock)?.resource_address();
                    if vault_resource != resource_address {
                        return Err(RuntimeError::RecallResourceMismatch {
                            vault_id: arg.vault_id,
                            resource_address,
                            vault_resource,
                        });
                    }

                    let resource = state_mut.recall_resource_from_vault(&vault_lock, &arg.resource)?;

                    let payload = try_metadata!(
                        "vault_id" => arg.vault_id,
                        "recall_desc" => arg.resource,
                    )?;
                    Self::emit_std_event("resource", "recall", resource_address, payload, state_mut)?;

                    let moved = resource.non_fungible_token_ids().len();
                    let bucket_id = state_mut.id_provider()?.new_bucket_id()?;
                    state_mut.new_bucket(bucket_id, resource)?;

                    state_mut.unlock_substate(vault_lock)?;

                    let bucket = tari_template_lib::models::Bucket::from_id(bucket_id);
                    Ok::<_, RuntimeError>((InvokeResult::encode(&bucket)?, moved))
                })?;
                self.charge_non_fungible_ids_moved(moved)?;

                Ok(result)
            },
            ResourceAction::GetNonFungible => {
                let resource_address =
                    resource_ref
                        .as_resource_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "GetNonFungible resource action requires a resource address".to_string(),
                        })?;
                let arg: ResourceGetNonFungibleArg = args.assert_one_arg()?;

                self.tracker.write_with(|state| {
                    let nft_addr = NonFungibleAddress::new(resource_address, arg.id.clone());
                    let addr = SubstateId::NonFungible(nft_addr.clone());
                    let nft_lock = state.read_lock_substate(addr)?;

                    let nf_container = state.get_non_fungible(&nft_lock)?;

                    if nf_container.is_burnt() {
                        return Err(RuntimeError::InvalidOpNonFungibleBurnt {
                            op: "GetNonFungible",
                            nf_id: arg.id,
                            resource_address,
                        });
                    }

                    state.unlock_substate(nft_lock)?;

                    Ok(InvokeResult::encode(&nft_addr)?)
                })
            },
            ResourceAction::UpdateNonFungibleData => {
                let resource_address =
                    resource_ref
                        .as_resource_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "UpdateNonFungibleData resource action requires a resource address".to_string(),
                        })?;
                let arg: ResourceUpdateNonFungibleDataArg = args.assert_one_arg()?;
                reject_transient_values_in("Non-fungible mutable data", &arg.data)?;

                let (maybe_auth_hook, auth_caller) = self.tracker.write_with_authorization(|state_mut| {
                    let resource_lock = state_mut.read_lock_substate(SubstateId::Resource(resource_address))?;

                    let resource = state_mut.get_resource(&resource_lock)?;

                    state_mut.authorization().check_resource_access_rules(
                        ResourceAuthAction::UpdateNonFungibleData,
                        resource.as_ownership(),
                        resource.access_rules(),
                    )?;

                    let auth_hook = resource.auth_hook().cloned();
                    let auth_caller = state_mut.get_auth_caller(&resource_lock)?;

                    state_mut.unlock_substate(resource_lock)?;
                    Ok::<_, RuntimeError>((auth_hook, auth_caller))
                })?;

                if let Some(auth_hook) = maybe_auth_hook {
                    self.invoke_resource_access_hook(
                        auth_hook,
                        auth_caller,
                        ResourceAuthAction::UpdateNonFungibleData,
                    )?;
                }

                self.tracker.write_with(|state_mut| {
                    let addr = NonFungibleAddress::new(resource_address, arg.id);
                    let locked = state_mut.write_lock_substate(SubstateId::NonFungible(addr.clone()))?;

                    let nft = state_mut.get_non_fungible_mut(&locked)?;

                    let contents = nft
                        .contents_mut()
                        .ok_or_else(|| RuntimeError::InvalidOpNonFungibleBurnt {
                            op: "UpdateNonFungibleData",
                            resource_address,
                            nf_id: addr.id().clone(),
                        })?;
                    contents.set_mutable_data(arg.data);

                    Self::emit_std_event(
                        "resource",
                        "update_nonfungible_data",
                        resource_address,
                        Metadata::new(),
                        state_mut,
                    )?;

                    state_mut.unlock_substate(locked)?;

                    Ok(InvokeResult::unit())
                })
            },
            ResourceAction::UpdateAccessRule => {
                let resource_address =
                    resource_ref
                        .as_resource_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "UpdateAccessRule resource action requires a resource address".to_string(),
                        })?;
                let UpdateAccessRuleArg { action, new_rule } = args.assert_one_arg()?;
                reject_invalid_rule("new_rule", new_rule.find_invalid_rule())?;

                let resource_lock = self.tracker.write_with_authorization(|state_mut| {
                    let resource_lock = state_mut.read_lock_substate(SubstateId::Resource(resource_address))?;

                    let resource = state_mut.get_resource(&resource_lock)?;
                    let updater = resource.access_rules().get_updater(&action);

                    let authorized = match updater {
                        UpdateRule::Locked => false,
                        UpdateRule::Owner => state_mut.authorization().check_ownership(resource.as_ownership())?,
                        UpdateRule::AccessRule(rule) => state_mut.authorization().check_access_rule(rule)?,
                    };

                    if !authorized {
                        return Err(RuntimeError::AccessDenied {
                            action_ident: ActionIdent::Native(NativeAction::UpdateResourceAccessRule(action)),
                        });
                    }

                    state_mut.unlock_substate(resource_lock)?;
                    state_mut.write_lock_substate(SubstateId::Resource(resource_address))
                })?;

                self.tracker.write_with(|state_mut| {
                    let resource_mut = state_mut.get_resource_mut(&resource_lock)?;
                    resource_mut.update_access_rule(action, new_rule);
                    let payload = try_metadata!("action" => format!("{:?}", action))?;
                    Self::emit_std_event("resource", "update_access_rule", resource_address, payload, state_mut)?;

                    state_mut.unlock_substate(resource_lock)?;

                    Ok(InvokeResult::unit())
                })
            },
            ResourceAction::UpdateAuthHook => {
                let resource_address =
                    resource_ref
                        .as_resource_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "UpdateAuthHook resource action requires a resource address".to_string(),
                        })?;
                let UpdateAuthHookArg { auth_hook } = args.assert_one_arg()?;

                let resource_lock = self.tracker.write_with_authorization(|state_mut| {
                    let resource_lock = state_mut.read_lock_substate(SubstateId::Resource(resource_address))?;

                    let resource = state_mut.get_resource(&resource_lock)?;
                    let updater = resource.access_rules().auth_hook_updater();

                    let authorized = match updater {
                        UpdateRule::Locked => false,
                        UpdateRule::Owner => state_mut.authorization().check_ownership(resource.as_ownership())?,
                        UpdateRule::AccessRule(rule) => state_mut.authorization().check_access_rule(rule)?,
                    };

                    if !authorized {
                        return Err(RuntimeError::AccessDenied {
                            action_ident: ActionIdent::Native(NativeAction::UpdateResourceAuthHook),
                        });
                    }

                    state_mut.unlock_substate(resource_lock)?;
                    state_mut.write_lock_substate(SubstateId::Resource(resource_address))
                })?;

                // The hook being replaced is not invoked: a hook that denies or panics is the failure this
                // action exists to repair, so asking it to approve its own removal would defeat the point.
                if let Some(hook) = auth_hook.as_ref() {
                    self.check_resource_auth_hook("UpdateAuthHookArg", hook)?;
                }

                self.tracker.write_with(|state_mut| {
                    let resource_mut = state_mut.get_resource_mut(&resource_lock)?;
                    // An absent key says the hook was removed, so a reader sees the removal without
                    // fetching the resource.
                    let mut payload = Metadata::new();
                    if let Some(hook) = auth_hook.as_ref() {
                        payload.try_insert("auth_hook", hook)?;
                    }
                    resource_mut.set_auth_hook(auth_hook);
                    Self::emit_std_event("resource", "update_auth_hook", resource_address, payload, state_mut)?;

                    state_mut.unlock_substate(resource_lock)?;

                    Ok(InvokeResult::unit())
                })
            },
            ResourceAction::UpdateMetadata => {
                let resource_address =
                    resource_ref
                        .as_resource_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "UpdateMetadata resource action requires a resource address".to_string(),
                        })?;
                let new_metadata: Metadata = args.assert_one_arg()?;

                Self::check_metadata_values("metadata", &new_metadata)?;
                Self::check_token_symbol(&new_metadata)?;

                let (maybe_auth_hook, auth_caller) = self.tracker.write_with_authorization(|state_mut| {
                    let resource_lock = state_mut.read_lock_substate(SubstateId::Resource(resource_address))?;

                    let resource = state_mut.get_resource(&resource_lock)?;

                    state_mut.authorization().check_resource_access_rules(
                        ResourceAuthAction::UpdateMetadata,
                        resource.as_ownership(),
                        resource.access_rules(),
                    )?;

                    // The token symbol is immutable once set.
                    if let Some(existing_symbol) = resource.token_symbol() &&
                        new_metadata.get_str(TOKEN_SYMBOL) != Some(existing_symbol)
                    {
                        return Err(RuntimeError::InvalidArgument {
                            argument: "metadata",
                            reason: "cannot update an existing token symbol".to_string(),
                        });
                    }

                    let auth_caller = state_mut.get_auth_caller(&resource_lock)?;
                    let auth_hook = resource.auth_hook().cloned();
                    state_mut.unlock_substate(resource_lock)?;
                    Ok::<_, RuntimeError>((auth_hook, auth_caller))
                })?;

                if let Some(auth_hook) = maybe_auth_hook {
                    self.invoke_resource_access_hook(auth_hook, auth_caller, ResourceAuthAction::UpdateMetadata)?;
                }

                let resource_lock = self.relock_resource(resource_address, LockFlag::Write)?;

                self.tracker.write_with(|state_mut| {
                    let resource_mut = state_mut.get_resource_mut(&resource_lock)?;
                    resource_mut.set_metadata(new_metadata);
                    Self::emit_std_event(
                        "resource",
                        "update_metadata",
                        resource_address,
                        Metadata::new(),
                        state_mut,
                    )?;

                    state_mut.unlock_substate(resource_lock)?;

                    Ok(InvokeResult::unit())
                })
            },
            ResourceAction::SetVaultFreeze => {
                let resource_address =
                    resource_ref
                        .as_resource_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "Freeze resource action requires a resource address".to_string(),
                        })?;
                let arg: FreezeResourceArg = args.assert_one_arg()?;

                if !arg.flags.validate() {
                    return Err(RuntimeError::InvalidArgument {
                        argument: "FreezeResourceArg",
                        reason: "Invalid freeze flags".to_string(),
                    });
                }

                let (maybe_auth_hook, auth_caller) = self.tracker.write_with_authorization(|state_mut| {
                    let resource_lock = state_mut.read_lock_substate(SubstateId::Resource(resource_address))?;

                    let resource = state_mut.get_resource(&resource_lock)?;

                    state_mut.authorization().check_resource_access_rules(
                        ResourceAuthAction::Freeze,
                        resource.as_ownership(),
                        resource.access_rules(),
                    )?;

                    let auth_hook = resource.auth_hook().cloned();
                    let auth_caller = state_mut.get_auth_caller(&resource_lock)?;

                    state_mut.unlock_substate(resource_lock)?;
                    Ok::<_, RuntimeError>((auth_hook, auth_caller))
                })?;

                if let Some(auth_hook) = maybe_auth_hook {
                    self.invoke_resource_access_hook(auth_hook, auth_caller, ResourceAuthAction::Freeze)?;
                }

                self.tracker.write_with(|state_mut| {
                    let vault_lock = state_mut.write_lock_substate(arg.vault_id.into())?;

                    // The freeze rule that authorized this action belongs to `resource_address`, so it may only
                    // reach vaults holding that resource.
                    let vault_resource = *state_mut.get_vault(&vault_lock)?.resource_address();
                    if vault_resource != resource_address {
                        return Err(RuntimeError::FreezeResourceMismatch {
                            vault_id: arg.vault_id,
                            resource_address,
                            vault_resource,
                        });
                    }

                    state_mut.set_vault_freeze(&vault_lock, arg.flags)?;
                    let payload = try_metadata!("vault_id" => arg.vault_id, "flags" => arg.flags.to_string())?;
                    let action = if arg.flags.is_empty() { "unfreeze" } else { "freeze" };
                    Self::emit_std_event("resource", action, resource_address, payload, state_mut)?;

                    state_mut.unlock_substate(vault_lock)?;

                    Ok(InvokeResult::unit())
                })
            },
            ResourceAction::StealthTransfer => {
                let resource_address =
                    resource_ref
                        .as_resource_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "StealthTransfer resource action requires a resource address".to_string(),
                        })?;
                let arg: StealthTransferResourceArg = args.assert_one_arg()?;
                let maybe_bucket = self.stealth_transfer(resource_address.into(), arg.transfer, arg.input_bucket)?;
                Ok(InvokeResult::encode(
                    &maybe_bucket.map(tari_template_lib::models::Bucket::from_id),
                )?)
            },
            ResourceAction::SetStealthUtxosFreeze => {
                let resource_address =
                    resource_ref
                        .as_resource_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "FreezeStealthUtxo resource action requires a resource address".to_string(),
                        })?;
                let arg: SetFreezeStealthUtxosArg = args.assert_one_arg()?;

                if arg.utxos.is_empty() {
                    return Err(RuntimeError::InvalidArgument {
                        argument: "SetFreezeStealthUtxosArg",
                        reason: "Utxos list cannot be empty".to_string(),
                    });
                }

                self.authorize_resource_action(resource_address, ResourceAuthAction::Freeze, |resource| {
                    if !resource.resource_type().is_stealth() {
                        return Err(RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "FreezeStealthUtxo can only be called on stealth resources".to_string(),
                        });
                    }
                    Ok(())
                })?;

                self.tracker.write_with(|state_mut| {
                    let op = if arg.freeze {
                        "freeze_stealth_utxo"
                    } else {
                        "unfreeze_stealth_utxo"
                    };
                    for utxo in arg.utxos {
                        let id = SubstateId::Utxo(UtxoAddress::new(resource_address, utxo));
                        let locked = state_mut.write_lock_substate(id.clone())?;

                        let utxo_mut = state_mut.get_utxo_mut(&locked)?;
                        if utxo_mut.is_burnt() {
                            state_mut.unlock_substate(locked)?;
                            return Err(RuntimeError::InvalidOpStealthUtxoBurnt {
                                op,
                                resource_address,
                                utxo_id: utxo,
                            });
                        }

                        // Freeze is idempotent.
                        if arg.freeze {
                            utxo_mut.freeze();
                        } else {
                            utxo_mut.unfreeze();
                        }
                        state_mut.unlock_substate(locked)?;
                    }

                    Ok(InvokeResult::unit())
                })
            },
            ResourceAction::SetConfidentialOutputsFreeze => {
                let resource_address =
                    resource_ref
                        .as_resource_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "SetConfidentialOutputsFreeze resource action requires a resource address"
                                .to_string(),
                        })?;
                let arg: SetFreezeConfidentialOutputsArg = args.assert_one_arg()?;

                if arg.commitments.is_empty() {
                    return Err(RuntimeError::InvalidArgument {
                        argument: "SetFreezeConfidentialOutputsArg",
                        reason: "Commitments list cannot be empty".to_string(),
                    });
                }

                self.authorize_resource_action(resource_address, ResourceAuthAction::Freeze, |resource| {
                    if !resource.resource_type().is_confidential() {
                        return Err(RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "SetConfidentialOutputsFreeze can only be called on confidential resources"
                                .to_string(),
                        });
                    }
                    Ok(())
                })?;

                self.tracker.write_with(|state_mut| {
                    for commitment in arg.commitments {
                        let id = SubstateId::ConfidentialOutput(ConfidentialOutputAddress::new(
                            resource_address,
                            commitment,
                        ));
                        let locked = state_mut.write_lock_substate(id.clone())?;

                        let output_mut = state_mut.get_confidential_output_mut(&locked)?;

                        // Freeze is idempotent.
                        if arg.freeze {
                            output_mut.freeze();
                        } else {
                            output_mut.unfreeze();
                        }
                        state_mut.unlock_substate(locked)?;
                    }

                    Ok(InvokeResult::unit())
                })
            },
            ResourceAction::StealthUtxoBurn => {
                let resource_address =
                    resource_ref
                        .as_resource_address()
                        .ok_or_else(|| RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "BurnStealthUtxo resource action requires a resource address".to_string(),
                        })?;
                let arg: BurnStealthUtxoArg = args.assert_one_arg()?;

                // Charge the value proof's native verification (a Schnorr/ElGamal check) against
                // the payment-funded allowance before it runs.
                self.tracker
                    .charge_native_execution(crypto::value_proof_native_points(&arg.value_proof))?;

                self.authorize_resource_action(resource_address, ResourceAuthAction::Burn, |resource| {
                    if !resource.resource_type().is_stealth() {
                        return Err(RuntimeError::InvalidArgument {
                            argument: "resource_ref",
                            reason: "StealthUtxoBurn can only be called on stealth resources".to_string(),
                        });
                    }
                    Ok(())
                })?;

                self.tracker.write_with(|state_mut| {
                    let resource_lock = state_mut.read_lock_substate(SubstateId::Resource(resource_address))?;
                    let resource = state_mut.get_resource(&resource_lock)?;
                    let is_total_supply_tracking_enabled = resource.is_supply_tracking_enabled();
                    let maybe_view_key =
                        resource
                            .to_view_key_public_key()
                            .map_err(|e| RuntimeError::InvariantError {
                                function: "ResourceAction::StealthUtxoBurn",
                                details: format!(
                                    "Resource {} has a malformed view key: {}",
                                    resource_lock.substate_id(),
                                    e
                                ),
                            })?;
                    state_mut.unlock_substate(resource_lock)?;

                    let id = SubstateId::Utxo(UtxoAddress::new(resource_address, arg.utxo_id));
                    let utxo_lock = state_mut.write_lock_substate(id.clone())?;

                    let utxo_mut = state_mut.get_utxo_mut(&utxo_lock)?;

                    if utxo_mut.is_burnt() {
                        return Err(RuntimeError::ResourceError(ResourceError::UtxoBurnFailed {
                            id: arg.utxo_id,
                            details: "already burnt".to_string(),
                        }));
                    }

                    if utxo_mut.is_frozen() {
                        return Err(RuntimeError::ResourceError(ResourceError::UtxoBurnFailed {
                            id: arg.utxo_id,
                            details: "frozen".to_string(),
                        }));
                    }

                    let commitment = arg.utxo_id.into_commitment_bytes();

                    // The output's creator chose its mask, so a mask-knowledge proof shows knowledge of the opening
                    // but not ownership. It authorises a burn only together with the UTXO's key-path spend authority.
                    // A view-key proof needs the resource's secret view key and stands on its own.
                    if matches!(arg.value_proof.knowledge_proof, ValueKnowledgeProof::Commitment { .. }) {
                        let Some(spend_key) = utxo_mut.output().and_then(|o| o.auth.spend_key()).copied() else {
                            return Err(RuntimeError::ResourceError(ResourceError::UtxoBurnFailed {
                                id: arg.utxo_id,
                                details: "a mask-knowledge value proof can only burn a UTXO that has a spend key"
                                    .to_string(),
                            }));
                        };
                        let badge = NonFungibleAddress::from_public_key(spend_key);
                        if !state_mut.base_call_scope().auth_scope().contains_badge(&badge) {
                            return Err(RuntimeError::ResourceError(
                                ResourceError::RequiredSignatureMissingForStealthUtxo {
                                    commitment,
                                    public_key: spend_key,
                                },
                            ));
                        }
                    }

                    let utxo_mut = state_mut.get_utxo_mut(&utxo_lock)?;
                    // The value proof is required whether or not the resource tracks its total supply, since it is
                    // what limits burns to the UTXO owner or the view-key holder.
                    // Burning discards the output, so the proof is validated against the viewable balance first
                    let elgamal_proof = utxo_mut
                        .output
                        .as_ref()
                        .and_then(|o| o.output.viewable_balance.as_ref());
                    let value = crypto::validate_value_proof(
                        &commitment,
                        maybe_view_key.as_ref(),
                        elgamal_proof,
                        &arg.value_proof,
                    )?;
                    utxo_mut.burn();
                    if is_total_supply_tracking_enabled && value.is_positive() {
                        let resource_lock = state_mut.write_lock_substate(SubstateId::Resource(resource_address))?;
                        state_mut.decrease_total_supply(&resource_lock, value)?;
                        state_mut.unlock_substate(resource_lock)?;
                    }

                    state_mut.unlock_substate(utxo_lock)?;

                    Ok(InvokeResult::unit())
                })
            },
        }
    }

    #[allow(clippy::too_many_lines)]
    fn vault_invoke(
        &self,
        vault_ref: VaultRef,
        action: VaultAction,
        args: EngineArgs,
    ) -> Result<InvokeResult, RuntimeError> {
        self.invoke_modules_on_runtime_call("vault_invoke")?;

        trace!(target: LOG_TARGET, "Vault invoke: {} {:?}", vault_ref, action,);

        // Check vault ownership if referencing an ID
        if action.requires_write_access() &&
            let Some(vault_id) = vault_ref.vault_id()
        {
            self.tracker
                .read_with(|state| state.check_component_scope(&vault_id.into(), action))?;
        }

        match action {
            VaultAction::Create => {
                let resource_address = vault_ref
                    .resource_address()
                    .ok_or_else(|| RuntimeError::InvalidArgument {
                        argument: "vault_ref",
                        reason: "Create vault action requires a resource address".to_string(),
                    })?;
                args.assert_no_args("CreateVault")?;

                self.tracker.write_with_authorization(|state| {
                    let resource_substate_id = SubstateId::Resource(*resource_address);
                    let resource_lock = state.read_lock_substate(resource_substate_id.clone())?;
                    let resource = state.get_resource(&resource_lock)?;

                    // Require deposit permissions on the resource to create the vault (even if empty)
                    state.authorization().check_resource_access_rules(
                        ResourceAuthAction::Deposit,
                        resource.as_ownership(),
                        resource.access_rules(),
                    )?;

                    let resource_type = state.get_resource(&resource_lock)?.resource_type();
                    let vault_id = state.id_provider()?.new_vault_id()?;
                    let resource = match resource_type {
                        ResourceType::Fungible => ResourceContainer::public_fungible(*resource_address, Amount::zero()),
                        ResourceType::NonFungible => {
                            ResourceContainer::non_fungible(*resource_address, Default::default())
                        },
                        ResourceType::Confidential => {
                            ResourceContainer::confidential(*resource_address, None, Amount::zero())
                        },
                        ResourceType::Stealth => ResourceContainer::stealth(*resource_address, Amount::zero()),
                    };

                    let vault = Vault::new(resource);

                    state.new_substate(vault_id, vault)?;
                    debug!(
                        target: LOG_TARGET,
                        "Created vault {} for resource {} ({})",
                        vault_id,
                        resource_address,
                        resource_type
                    );
                    state.unlock_substate(resource_lock)?;

                    // The resource is not orphaned because of the new vault.
                    state
                        .current_call_scope_mut()?
                        .move_node_to_owned(&resource_substate_id)?;

                    Ok(InvokeResult::encode(&vault_id)?)
                })
            },
            VaultAction::Deposit => {
                let vault_id = vault_ref.vault_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "vault_ref",
                    reason: "Put vault action requires a vault id".to_string(),
                })?;

                let bucket_id: BucketId = args.assert_one_arg()?;

                self.authorize_vault_action(vault_id, ResourceAuthAction::Deposit, VaultFreezeFlag::Deposits)?;

                let moved = self.tracker.write_with(move |state_mut| {
                    let vault_lock = state_mut.write_lock_substate(SubstateId::Vault(vault_id))?;
                    let bucket = state_mut.take_bucket(bucket_id)?;
                    Self::check_bucket_is_unlocked("deposit", bucket_id, &bucket)?;

                    // Emit a builtin event for the deposit
                    let payload = try_metadata!("amount" => bucket.unlocked_amount())?;

                    Self::emit_std_event("vault", "deposit", vault_id, payload, state_mut)?;

                    let vault_mut = state_mut.get_vault_mut(&vault_lock)?;

                    let moved = bucket.non_fungible_ids().len();
                    vault_mut.deposit(bucket)?;

                    state_mut.unlock_substate(vault_lock)?;

                    Ok::<_, RuntimeError>(moved)
                })?;
                self.charge_non_fungible_ids_moved(moved)?;

                Ok(InvokeResult::unit())
            },
            VaultAction::Withdraw => {
                let vault_id = vault_ref.vault_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "vault_ref",
                    reason: "Withdraw vault action requires a vault id".to_string(),
                })?;
                let arg: VaultWithdrawArg = args.assert_one_arg()?;

                // Enforce confidential limits before substate reads, authorization hooks, or proof crypto run.
                if let VaultWithdrawArg::Confidential { proof } = &arg {
                    self.tracker.write_with(|state| {
                        state.check_write_allowed(&SubstateId::Vault(vault_id))?;
                        state.admit_confidential_withdraw(proof)
                    })?;
                }

                let resource_address =
                    self.authorize_vault_action(vault_id, ResourceAuthAction::Withdraw, VaultFreezeFlag::Withdrawals)?;

                // Charge the withdraw's native verification cost against the payment-funded
                // allowance before any of its proof crypto (including view-key decompression) runs.
                let maybe_view_key =
                    if let VaultWithdrawArg::Confidential { proof } = &arg {
                        let view_key_bytes = self.tracker.write_with(|state_mut| {
                            let resource_lock = state_mut.read_lock_substate(SubstateId::Resource(resource_address))?;
                            let view_key_bytes = state_mut.get_resource(&resource_lock)?.view_key().copied();
                            state_mut.unlock_substate(resource_lock)?;
                            Ok::<_, RuntimeError>(view_key_bytes)
                        })?;
                        self.tracker.charge_native_execution(
                            tari_engine_types::confidential::withdraw_native_points(proof, view_key_bytes.is_some()),
                        )?;
                        // The key is read only to verify the confidential outputs' viewable-balance proofs, which
                        // `PER_OUTPUT_VIEWABLE_SURCHARGE` in the charge above pays for.
                        view_key_bytes
                            .filter(|_| proof.output_proof.num_confidential_outputs() > 0)
                            .map(|bytes| {
                                RistrettoPublicKey::from_canonical_bytes(bytes.as_bytes()).map_err(|e| {
                                    RuntimeError::InvariantError {
                                        function: "VaultAction::Withdraw",
                                        details: format!(
                                            "Resource {} has a malformed view key: {}",
                                            SubstateId::Resource(resource_address),
                                            e
                                        ),
                                    }
                                })
                            })
                            .transpose()?
                    } else {
                        None
                    };

                let (result, moved) = self.tracker.write_with(|state| {
                    let vault_lock = state.write_lock_substate(SubstateId::Vault(vault_id))?;

                    let vault_mut = state.get_vault_mut(&vault_lock)?;
                    let (resource_container, public_amount, confidential_effects) = match arg {
                        VaultWithdrawArg::Fungible { amount } | VaultWithdrawArg::Stealth { amount } => {
                            let container = vault_mut.withdraw(amount)?;
                            (container, amount, None)
                        },
                        VaultWithdrawArg::NonFungible { ids } => {
                            let container = vault_mut.withdraw_non_fungibles(&ids)?;
                            let amount = ids.len() as u128;
                            (container, amount.into(), None)
                        },
                        VaultWithdrawArg::Confidential { proof } => {
                            let amount = proof.revealed_input_amount();
                            let (container, effects) =
                                vault_mut.withdraw_confidential(*proof, maybe_view_key.as_ref())?;
                            (container, amount, Some(effects))
                        },
                    };

                    // Down the spent input substates and materialise the new change/output commitments.
                    if let Some(effects) = confidential_effects {
                        let resource_address = *resource_container.resource_address();
                        state.materialize_confidential_outputs(resource_address, effects)?;
                    }

                    // Emit a builtin event for the withdraw
                    let payload = try_metadata!("amount" => public_amount)?;

                    Self::emit_std_event("vault", "withdraw", vault_id, payload, state)?;

                    let moved = resource_container.non_fungible_token_ids().len();
                    let bucket_id = state.id_provider()?.new_bucket_id()?;
                    state.new_bucket(bucket_id, resource_container)?;

                    state.unlock_substate(vault_lock)?;

                    let bucket = tari_template_lib::models::Bucket::from_id(bucket_id);
                    Ok::<_, RuntimeError>((InvokeResult::encode(&bucket)?, moved))
                })?;
                self.charge_non_fungible_ids_moved(moved)?;

                Ok(result)
            },
            VaultAction::GetBalance => {
                let vault_id = vault_ref.vault_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "vault_ref",
                    reason: "GetBalance vault action requires a vault id".to_string(),
                })?;
                args.assert_no_args("Vault::GetBalance")?;

                self.tracker.write_with(|state| {
                    let vault_lock = state.read_lock_substate(SubstateId::Vault(vault_id))?;
                    let balance = state.get_vault(&vault_lock)?.balance();
                    state.unlock_substate(vault_lock)?;
                    Ok(InvokeResult::encode(&balance)?)
                })
            },
            VaultAction::GetLockedBalance => {
                let vault_id = vault_ref.vault_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "vault_ref",
                    reason: "GetBalance vault action requires a vault id".to_string(),
                })?;
                args.assert_no_args("Vault::GetBalance")?;

                self.tracker.write_with(|state| {
                    let vault_lock = state.read_lock_substate(SubstateId::Vault(vault_id))?;
                    let balance = state.get_vault(&vault_lock)?.locked_balance();
                    state.unlock_substate(vault_lock)?;
                    Ok(InvokeResult::encode(&balance)?)
                })
            },
            VaultAction::GetResourceAddress => {
                let vault_id = vault_ref.vault_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "vault_ref",
                    reason: "vault action requires a vault id".to_string(),
                })?;
                args.assert_no_args("Vault::GetResourceAddress")?;

                self.tracker.write_with(|state| {
                    let vault_lock = state.read_lock_substate(SubstateId::Vault(vault_id))?;
                    let resource_address = *state.get_vault(&vault_lock)?.resource_address();
                    state.unlock_substate(vault_lock)?;
                    Ok(InvokeResult::encode(&resource_address)?)
                })
            },
            VaultAction::GetNonFungibleIds => {
                let vault_id = vault_ref.vault_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "vault_ref",
                    reason: "vault action requires a vault id".to_string(),
                })?;
                args.assert_no_args("Vault::GetNonFungibleIds")?;

                self.tracker.write_with(|state| {
                    let vault_lock = state.read_lock_substate(SubstateId::Vault(vault_id))?;
                    let non_fungible_ids = state.get_vault(&vault_lock)?.get_non_fungible_ids();
                    let result = InvokeResult::encode(&non_fungible_ids)?;
                    state.unlock_substate(vault_lock)?;
                    Ok(result)
                })
            },
            VaultAction::GetCommitmentCount => {
                let vault_id = vault_ref.vault_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "vault_ref",
                    reason: "vault action requires a vault id".to_string(),
                })?;

                args.assert_no_args("Vault::GetCommitmentCount")?;

                self.tracker.write_with(|state| {
                    let vault_lock = state.read_lock_substate(SubstateId::Vault(vault_id))?;
                    let commitment_count = state.get_vault(&vault_lock)?.get_commitment_count();
                    state.unlock_substate(vault_lock)?;
                    Ok(InvokeResult::encode(&commitment_count)?)
                })
            },
            VaultAction::PayFee => {
                let vault_id = vault_ref.vault_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "vault_ref",
                    reason: "PayFee vault action requires a vault id".to_string(),
                })?;

                let arg: PayFeeArg = args.assert_one_arg()?;

                if self.tracker.is_fee_intent_checkpointed() {
                    return Err(RuntimeError::FeePaymentInMainIntent);
                }

                self.tracker
                    .read_with(|state| state.check_write_allowed(&SubstateId::Vault(vault_id)))?;

                // Charge for the statement's verification cost and authorise the spent inputs before the fee transfer
                // executes — the same mandatory pre-execute gate as `stealth_transfer`, so a rejection leaves the
                // inputs unspent. Fees are always paid in TARI.
                let admitted_transfer = match arg.statement {
                    Some(statement) => {
                        self.tracker.admit_fee_intent_stealth_transfer()?;
                        let admitted = self
                            .tracker
                            .write_with(|state| state.admit_stealth_transfer(statement))?;
                        // TARI has no view key, so the viewable-balance surcharge never applies to a fee payment.
                        self.tracker
                            .charge_native_execution(stealth::transfer_native_points(admitted.statement(), false))?;
                        self.verify_input_authorizations(TARI_TOKEN.into(), admitted.statement())?;
                        Some(admitted)
                    },
                    None => None,
                };

                self.tracker.write_with_authorization(|state_mut| {
                    let vault_lock = state_mut.write_lock_substate(SubstateId::Vault(vault_id))?;

                    let vault = state_mut.get_vault(&vault_lock)?;
                    if vault.freeze_flags().contains(VaultFreezeFlag::Withdrawals) {
                        return Err(RuntimeError::VaultFrozen {
                            vault_id,
                            freeze_flag: VaultFreezeFlag::Withdrawals,
                        });
                    }
                    let resource_address = vault.resource_address();
                    if *resource_address != TARI_TOKEN {
                        return Err(RuntimeError::InvalidArgument {
                            argument: "vault_ref",
                            reason: format!(
                                "Fees can only be paid using XTR, however the vault contained resource {}",
                                resource_address
                            ),
                        });
                    }
                    let resource_lock = state_mut.read_lock_substate(SubstateId::Resource(TARI_TOKEN))?;
                    let resource = state_mut.get_resource(&resource_lock)?;

                    state_mut.authorization().check_resource_access_rules(
                        ResourceAuthAction::Withdraw,
                        resource.as_ownership(),
                        resource.access_rules(),
                    )?;

                    let vault_mut = state_mut.get_vault_mut(&vault_lock)?;

                    let mut container = ResourceContainer::stealth(TARI_TOKEN, Amount::zero());
                    if arg.amount.is_positive() {
                        let withdrawn = vault_mut.withdraw(arg.amount)?;
                        container.deposit(withdrawn)?;
                    }
                    if let Some(admitted) = admitted_transfer &&
                        let Some(revealed) = state_mut.execute_stealth_transfer(TARI_TOKEN.into(), admitted, None)?
                    {
                        container.deposit(revealed)?;
                    }
                    if container.unlocked_amount().is_zero() {
                        return Err(RuntimeError::InvalidArgument {
                            argument: "TakeFeesArg",
                            reason: "Fee payment has zero value".to_string(),
                        });
                    }

                    Self::emit_std_event(
                        "vault",
                        "pay_fee",
                        vault_id,
                        try_metadata!("amount" => container.unlocked_amount())?,
                        state_mut,
                    )?;

                    state_mut.pay_fee(container, Some(vault_id))?;

                    state_mut.unlock_substate(resource_lock)?;
                    state_mut.unlock_substate(vault_lock)?;

                    Ok(InvokeResult::unit())
                })
            },
            VaultAction::CreateProofByResource => {
                let vault_id = vault_ref.vault_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "vault_ref",
                    reason: "CreateProofByResource vault action requires a vault id".to_string(),
                })?;
                args.assert_no_args("CreateProofByResource")?;

                self.authorize_vault_action(vault_id, ResourceAuthAction::Withdraw, VaultFreezeFlag::Withdrawals)?;

                let (result, locked) = self.tracker.write_with(|state| {
                    let vault_lock = state.write_lock_substate(SubstateId::Vault(vault_id))?;
                    let proof_id = state.id_provider()?.new_proof_id()?;
                    let vault_mut = state.get_vault_mut(&vault_lock)?;
                    let locked_funds = vault_mut.lock_all(vault_id)?;
                    let locked = locked_funds.non_fungible_token_ids().len();
                    state.new_proof(proof_id, locked_funds)?;

                    state.unlock_substate(vault_lock)?;

                    Ok::<_, RuntimeError>((InvokeResult::encode(&proof_id)?, locked))
                })?;
                self.charge_non_fungible_ids_locked(locked)?;

                Ok(result)
            },
            VaultAction::CreateProofByFungibleAmount => {
                let vault_id = vault_ref.vault_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "vault_ref",
                    reason: "CreateProofByFungibleAmount vault action requires a vault id".to_string(),
                })?;
                let arg: VaultCreateProofByFungibleAmountArg = args.assert_one_arg()?;

                self.authorize_vault_action(vault_id, ResourceAuthAction::Withdraw, VaultFreezeFlag::Withdrawals)?;

                let (result, locked) = self.tracker.write_with(|state| {
                    let vault_lock = state.write_lock_substate(SubstateId::Vault(vault_id))?;
                    let proof_id = state.id_provider()?.new_proof_id()?;
                    let vault_mut = state.get_vault_mut(&vault_lock)?;
                    let locked_funds = vault_mut.lock_by_amount(vault_id, arg.amount)?;
                    let locked = locked_funds.non_fungible_token_ids().len();
                    state.new_proof(proof_id, locked_funds)?;

                    state.unlock_substate(vault_lock)?;

                    Ok::<_, RuntimeError>((InvokeResult::encode(&proof_id)?, locked))
                })?;
                self.charge_non_fungible_ids_locked(locked)?;

                Ok(result)
            },
            VaultAction::CreateProofByNonFungibles => {
                let vault_id = vault_ref.vault_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "vault_ref",
                    reason: "CreateProofByNonFungibles vault action requires a vault id".to_string(),
                })?;
                let arg: VaultCreateProofByNonFungiblesArg = args.assert_one_arg()?;

                self.authorize_vault_action(vault_id, ResourceAuthAction::Withdraw, VaultFreezeFlag::Withdrawals)?;

                let (result, locked) = self.tracker.write_with(|state| {
                    let vault_lock = state.write_lock_substate(SubstateId::Vault(vault_id))?;
                    let proof_id = state.id_provider()?.new_proof_id()?;
                    let vault_mut = state.get_vault_mut(&vault_lock)?;
                    let locked_funds = vault_mut.lock_by_non_fungible_ids(vault_id, arg.ids)?;
                    let locked = locked_funds.non_fungible_token_ids().len();
                    state.new_proof(proof_id, locked_funds)?;

                    state.unlock_substate(vault_lock)?;

                    Ok::<_, RuntimeError>((InvokeResult::encode(&proof_id)?, locked))
                })?;
                self.charge_non_fungible_ids_locked(locked)?;

                Ok(result)
            },
            VaultAction::CreateProofByConfidentialResource => Err(RuntimeError::NotSupported {
                details: "CreateProofByConfidentialResource not implemented".to_string(),
            }),
            VaultAction::GetNonFungibles => {
                let vault_id = vault_ref.vault_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "vault_ref",
                    reason: "GetNonFungibles vault action requires a vault id".to_string(),
                })?;
                args.assert_no_args("Vault::GetNonFungibles")?;

                self.tracker.write_with(|state| {
                    let vault_lock = state.read_lock_substate(SubstateId::Vault(vault_id))?;
                    let resource_address = state.get_vault(&vault_lock)?.resource_address();
                    let nft_ids = state.get_vault(&vault_lock)?.get_non_fungible_ids();
                    let nfts: Vec<NonFungible> = nft_ids
                        .iter()
                        .map(|id| NonFungibleAddress::new(*resource_address, id.clone()))
                        .map(NonFungible::new)
                        .collect();

                    let result = InvokeResult::encode(&nfts)?;
                    state.unlock_substate(vault_lock)?;
                    Ok(result)
                })
            },
        }
    }

    #[allow(clippy::too_many_lines)]
    fn bucket_invoke(
        &self,
        bucket_ref: BucketRef,
        action: BucketAction,
        args: EngineArgs,
    ) -> Result<InvokeResult, RuntimeError> {
        self.invoke_modules_on_runtime_call("bucket_invoke")?;

        debug!(target: LOG_TARGET, "Bucket invoke: {} {:?}", bucket_ref, action,);

        match action {
            BucketAction::GetResourceAddress => {
                let bucket_id = bucket_ref.bucket_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "bucket_ref",
                    reason: "GetResourceAddress action requires a bucket id".to_string(),
                })?;
                args.assert_no_args("Bucket::GetResourceAddress")?;

                self.tracker.read_with(|state| {
                    let bucket = state.get_bucket(bucket_id)?;
                    Ok(InvokeResult::encode(bucket.resource_address())?)
                })
            },
            BucketAction::GetResourceType => {
                let bucket_id = bucket_ref.bucket_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "bucket_ref",
                    reason: "GetResourceType action requires a bucket id".to_string(),
                })?;
                args.assert_no_args("Bucket::GetResourceType")?;

                self.tracker.read_with(|state| {
                    let bucket = state.get_bucket(bucket_id)?;
                    Ok(InvokeResult::encode(&bucket.resource_type())?)
                })
            },
            BucketAction::GetAmount => {
                let bucket_id = bucket_ref.bucket_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "bucket_ref",
                    reason: "GetAmount bucket action requires a bucket id".to_string(),
                })?;

                let arg: BucketGetAmountArg = args.assert_one_arg()?;
                self.tracker.read_with(|state| {
                    let bucket = state.get_bucket(bucket_id)?;
                    match arg {
                        BucketGetAmountArg::AmountOnly => Ok(InvokeResult::encode(&bucket.unlocked_amount())?),
                        BucketGetAmountArg::LockedOnly => Ok(InvokeResult::encode(&bucket.locked_amount())?),
                        BucketGetAmountArg::AmountAndLocked => {
                            let amount = bucket
                                .unlocked_amount()
                                .checked_add(bucket.locked_amount())
                                .ok_or_else(|| RuntimeError::InvariantError {
                                    function: "BucketAction::GetAmount",
                                    details: "Total amount overflowed".to_string(),
                                })?;
                            Ok(InvokeResult::encode(&amount)?)
                        },
                        BucketGetAmountArg::Everything => {
                            let amount = bucket
                                .unlocked_amount()
                                .checked_add(bucket.locked_amount())
                                .and_then(|a| {
                                    a.checked_add(Amount::from_usize(bucket.number_of_confidential_commitments()))
                                })
                                .ok_or_else(|| RuntimeError::InvariantError {
                                    function: "BucketAction::GetAmount",
                                    details: "Total amount overflowed".to_string(),
                                })?;
                            Ok(InvokeResult::encode(&amount)?)
                        },
                    }
                })
            },
            BucketAction::Take => {
                let bucket_id = bucket_ref.bucket_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "bucket_ref",
                    reason: "Take bucket action requires a bucket id".to_string(),
                })?;
                let amount = args.assert_one_arg()?;

                let (result, moved) = self.tracker.write_with(|state| {
                    let bucket = state.get_bucket_mut(bucket_id)?;
                    let resource = bucket.take(amount)?;
                    let moved = resource.non_fungible_token_ids().len();
                    let bucket_id = state.new_bucket_id()?;
                    state.new_bucket(bucket_id, resource)?;
                    Ok::<_, RuntimeError>((InvokeResult::encode(&bucket_id)?, moved))
                })?;
                self.charge_non_fungible_ids_moved(moved)?;

                Ok(result)
            },
            BucketAction::TakeConfidential => {
                let bucket_id = bucket_ref.bucket_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "bucket_ref",
                    reason: "Take bucket action requires a bucket id".to_string(),
                })?;
                let proof = args.assert_one_arg()?;

                // Enforce confidential limits and charge the withdraw's native verification cost
                // against the payment-funded allowance before any of its proof crypto runs. The
                // resource peek is a cheap substate read that determines whether the
                // viewable-balance surcharge applies.
                let has_view_key = self.tracker.write_with(|state| {
                    state.admit_confidential_withdraw(&proof)?;
                    let bucket = state.get_bucket(bucket_id)?;
                    let resource_lock = state.read_lock_substate((*bucket.resource_address()).into())?;
                    let has_view_key = state.get_resource(&resource_lock)?.view_key().is_some();
                    state.unlock_substate(resource_lock)?;
                    Ok::<_, RuntimeError>(has_view_key)
                })?;
                self.tracker
                    .charge_native_execution(tari_engine_types::confidential::withdraw_native_points(
                        &proof,
                        has_view_key,
                    ))?;

                self.tracker.write_with(|state| {
                    let bucket = state.get_bucket(bucket_id)?;
                    let resource_lock = state.read_lock_substate((*bucket.resource_address()).into())?;
                    // The key is read only to verify the confidential outputs' viewable-balance proofs,
                    // which `PER_OUTPUT_VIEWABLE_SURCHARGE` in the charge above pays for.
                    let view_key = if proof.output_proof.num_confidential_outputs() > 0 {
                        state
                            .get_resource(&resource_lock)?
                            .to_view_key_public_key()
                            .map_err(|e| RuntimeError::InvariantError {
                                function: "BucketAction::TakeConfidential",
                                details: format!(
                                    "Resource {} has a malformed view key: {}",
                                    resource_lock.substate_id(),
                                    e
                                ),
                            })?
                    } else {
                        None
                    };
                    let bucket_mut = state.get_bucket_mut(bucket_id)?;
                    let (resource, effects) = bucket_mut.take_confidential(proof, view_key.as_ref())?;
                    let resource_address = *resource.resource_address();
                    state.materialize_confidential_outputs(resource_address, effects)?;
                    let bucket_id = state.id_provider()?.new_bucket_id()?;
                    state.new_bucket(bucket_id, resource)?;
                    state.unlock_substate(resource_lock)?;
                    Ok(InvokeResult::encode(&bucket_id)?)
                })
            },
            BucketAction::Join => {
                let bucket_id = bucket_ref.bucket_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "bucket_ref",
                    reason: "Join bucket action requires a bucket id".to_string(),
                })?;
                let other_bucket_id = args.assert_one_arg()?;

                let (result, moved) = self.tracker.write_with(|state| {
                    let other_bucket = state.take_bucket(other_bucket_id)?;
                    Self::check_bucket_is_unlocked("join", other_bucket_id, &other_bucket)?;
                    let moved = other_bucket.non_fungible_ids().len();
                    let bucket = state.get_bucket_mut(bucket_id)?;
                    bucket.join(other_bucket)?;
                    Ok::<_, RuntimeError>((InvokeResult::encode(&bucket_id)?, moved))
                })?;
                self.charge_non_fungible_ids_moved(moved)?;

                Ok(result)
            },
            BucketAction::Burn => {
                let bucket_id = bucket_ref.bucket_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "bucket_ref",
                    reason: "Burn bucket action requires a bucket id".to_string(),
                })?;

                let arg: BurnBucketArg = args.assert_one_arg()?;

                let (resource_address, maybe_auth_hook, auth_caller, tracks_supply) =
                    self.tracker.write_with_authorization(|state_mut| {
                        let bucket = state_mut.get_bucket(bucket_id)?;
                        // Reject a burn that cannot succeed before the auth hook runs or anything is charged for it.
                        // This is re-checked after the hook, which may lock funds itself.
                        Self::check_bucket_is_unlocked("burn", bucket_id, bucket)?;

                        let resource_address = *bucket.resource_address();
                        let resource_lock = state_mut.read_lock_substate(SubstateId::Resource(resource_address))?;

                        let resource = state_mut.get_resource(&resource_lock)?;

                        state_mut.authorization().check_resource_access_rules(
                            ResourceAuthAction::Burn,
                            resource.as_ownership(),
                            resource.access_rules(),
                        )?;

                        let auth_caller = state_mut.get_auth_caller(&resource_lock)?;
                        let auth_hook = resource.auth_hook().cloned();
                        let tracks_supply = resource.is_supply_tracking_enabled();
                        state_mut.unlock_substate(resource_lock)?;
                        Ok::<_, RuntimeError>((resource_address, auth_hook, auth_caller, tracks_supply))
                    })?;

                if let Some(auth_hook) = maybe_auth_hook {
                    self.invoke_resource_access_hook(auth_hook, auth_caller, ResourceAuthAction::Burn)?;
                }

                let resource_lock = self.relock_resource(
                    resource_address,
                    if tracks_supply { LockFlag::Write } else { LockFlag::Read },
                )?;

                // The hook may have altered the bucket, so it is re-inspected after the hook runs and before
                // anything is charged: a hook that locked funds makes the burn fail, and the charge must cover the
                // proofs actually verified below rather than those held when the hook was scheduled.
                let value_proof_points = self.tracker.write_with(|state_mut| {
                    let bucket = state_mut.get_bucket(bucket_id)?;
                    Self::check_bucket_is_unlocked("burn", bucket_id, bucket)?;
                    if !tracks_supply {
                        return Ok::<_, RuntimeError>(0);
                    }
                    // One proof is verified per unlocked commitment; the price depends on each proof's variant.
                    let points = bucket
                        .get_confidential_commitments()
                        .into_iter()
                        .flatten()
                        .filter_map(|commitment| arg.value_proofs.get(commitment))
                        .map(crypto::value_proof_native_points)
                        .fold(0u64, u64::saturating_add);
                    Ok(points)
                })?;

                // Charge the value proofs' native verification against the payment-funded allowance before they run.
                if value_proof_points > 0 {
                    self.tracker.charge_native_execution(value_proof_points)?;
                }

                self.tracker.write_with(|state| {
                    let bucket = state.take_bucket(bucket_id)?;

                    state.burn_bucket(bucket_id, bucket, &resource_lock, &arg.value_proofs)?;

                    state.unlock_substate(resource_lock)?;

                    Ok(InvokeResult::unit())
                })
            },
            BucketAction::CreateProof => {
                let bucket_id = bucket_ref.bucket_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "bucket_ref",
                    reason: "CreateProof bucket action requires a bucket id".to_string(),
                })?;

                args.assert_no_args("Bucket::CreateProof")?;

                let (maybe_auth_hook, auth_caller) = self.tracker.write_with_authorization(|state_mut| {
                    let bucket = state_mut.get_bucket(bucket_id)?;

                    let resource_lock =
                        state_mut.read_lock_substate(SubstateId::Resource(*bucket.resource_address()))?;

                    let resource = state_mut.get_resource(&resource_lock)?;

                    state_mut.authorization().check_resource_access_rules(
                        ResourceAuthAction::Withdraw,
                        resource.as_ownership(),
                        resource.access_rules(),
                    )?;

                    let auth_hook = resource.auth_hook().cloned();
                    let auth_caller = state_mut.get_auth_caller(&resource_lock)?;

                    state_mut.unlock_substate(resource_lock)?;
                    Ok::<_, RuntimeError>((auth_hook, auth_caller))
                })?;

                if let Some(auth_hook) = maybe_auth_hook {
                    self.invoke_resource_access_hook(auth_hook, auth_caller, ResourceAuthAction::Withdraw)?;
                }

                let (result, locked) = self.tracker.write_with(|state| {
                    let locked_funds = state.get_bucket_mut(bucket_id)?.lock_all()?;
                    let locked = locked_funds.non_fungible_token_ids().len();

                    let proof_id = state.id_provider()?.new_proof_id()?;
                    state.new_proof(proof_id, locked_funds)?;

                    Ok::<_, RuntimeError>((InvokeResult::encode(&proof_id)?, locked))
                })?;
                self.charge_non_fungible_ids_locked(locked)?;

                Ok(result)
            },
            BucketAction::GetNonFungibleIds => {
                let bucket_id = bucket_ref.bucket_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "bucket_ref",
                    reason: "GetNonFungibleIds bucket action requires a bucket id".to_string(),
                })?;
                args.assert_no_args("Bucket::GetNonFungibleIds")?;

                self.tracker.write_with(|state| {
                    let bucket = state.get_bucket(bucket_id)?;
                    Ok(InvokeResult::encode(bucket.non_fungible_ids())?)
                })
            },
            BucketAction::GetNonFungibles => {
                let bucket_id = bucket_ref.bucket_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "bucket_ref",
                    reason: "GetNonFungibles bucket action requires a bucket id".to_string(),
                })?;
                args.assert_no_args("Bucket::GetNonFungibles")?;

                self.tracker.write_with(|state| {
                    let bucket = state.get_bucket(bucket_id)?;
                    let resource_address = bucket.resource_address();
                    let nft_ids = bucket.non_fungible_ids();
                    let nfts: Vec<NonFungible> = nft_ids
                        .iter()
                        .map(|id| NonFungibleAddress::new(*resource_address, id.clone()))
                        .map(NonFungible::new)
                        .collect();

                    Ok(InvokeResult::encode(&nfts)?)
                })
            },
            BucketAction::CountConfidentialCommitments => {
                let bucket_id = bucket_ref.bucket_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "bucket_ref",
                    reason: "CountConfidentialCommitments bucket action requires a bucket id".to_string(),
                })?;
                args.assert_no_args("Bucket::CountConfidentialCommitments")?;

                self.tracker.write_with(|state| {
                    let bucket = state.get_bucket(bucket_id)?;
                    Ok(InvokeResult::encode(&bucket.number_of_confidential_commitments())?)
                })
            },
            BucketAction::DropEmpty => {
                let bucket_id = bucket_ref.bucket_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "bucket_ref",
                    reason: "DropEmpty bucket action requires a bucket id".to_string(),
                })?;
                args.assert_no_args("Bucket::DropEmpty")?;

                self.tracker.write_with(|state| {
                    let bucket = state.take_bucket(bucket_id)?;
                    if !bucket.is_empty() {
                        return Err(RuntimeError::InvalidArgument {
                            argument: "bucket_ref",
                            reason: "Cannot drop a non-empty bucket".to_string(),
                        });
                    }
                    // Drop
                    Ok(InvokeResult::unit())
                })
            },
        }
    }

    fn proof_invoke(
        &self,
        proof_ref: ProofRef,
        action: ProofAction,
        args: EngineArgs,
    ) -> Result<InvokeResult, RuntimeError> {
        self.invoke_modules_on_runtime_call("proof_invoke")?;

        debug!(
            target: LOG_TARGET,
            "Proof invoke: {} {:?}",
            proof_ref,
            action,
        );

        match action {
            ProofAction::GetAmount => {
                let proof_id = proof_ref.proof_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "proof_ref",
                    reason: "GetAmount proof action requires a proof id".to_string(),
                })?;
                args.assert_no_args("Proof.GetAmount")?;
                self.tracker.write_with(|state| {
                    let proof = state.get_proof_in_scope(proof_id)?;
                    Ok(InvokeResult::encode(&proof.amount())?)
                })
            },
            ProofAction::GetResourceAddress => {
                let proof_id = proof_ref.proof_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "proof_ref",
                    reason: "GetResourceAddress proof action requires a proof id".to_string(),
                })?;
                args.assert_no_args("Proof.GetResourceAddress")?;
                self.tracker.write_with(|state| {
                    let proof = state.get_proof_in_scope(proof_id)?;
                    Ok(InvokeResult::encode(proof.resource_address())?)
                })
            },
            ProofAction::GetResourceType => {
                let proof_id = proof_ref.proof_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "proof_ref",
                    reason: "GetResourceType proof action requires a proof id".to_string(),
                })?;

                args.assert_no_args("Proof.GetResourceType")?;

                self.tracker.write_with(|state| {
                    let proof = state.get_proof_in_scope(proof_id)?;
                    Ok(InvokeResult::encode(&proof.resource_type())?)
                })
            },
            ProofAction::GetNonFungibles => {
                let proof_id = proof_ref.proof_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "proof_ref",
                    reason: "GetNonFungibles proof action requires a proof id".to_string(),
                })?;

                args.assert_no_args("Proof.GetNonFungibles")?;

                self.tracker.write_with(|state| {
                    let proof = state.get_proof_in_scope(proof_id)?;
                    let nfts = proof.non_fungible_token_ids();
                    Ok(InvokeResult::encode(&nfts)?)
                })
            },
            ProofAction::Authorize => {
                let proof_id = proof_ref.proof_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "proof_ref",
                    reason: "Authorize proof action requires a proof id".to_string(),
                })?;
                args.assert_no_args("Proof.CreateAccess")?;

                self.tracker.write_with(|state| {
                    // A proof id is a sequential counter shared by the whole transaction, so authority has to come
                    // from a proof the frame holds: one it created or a callee handed back. A proof shown as an
                    // argument is the caller's to authorize, and every other live proof belongs to someone else.
                    // The answer is the same for all of them, so it reveals nothing about ids the frame cannot see.
                    // A window that is already open is refused too: the auth scope holds one entry per proof, so a
                    // second window would alias the first and close it when either guard drops.
                    let is_live = state.proof_exists(proof_id);
                    let scope = state.current_call_scope_mut()?;
                    if !is_live || !scope.is_proof_in_scope(&proof_id) || scope.auth_scope().contains_proof(&proof_id) {
                        return Ok(InvokeResult::encode(&Err::<(), _>(NotAuthorized))?);
                    }
                    scope.auth_scope_mut().add_proof(proof_id);
                    Ok(InvokeResult::encode(&Ok::<_, NotAuthorized>(()))?)
                })
            },
            ProofAction::DropAuthorize => {
                let proof_id = proof_ref.proof_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "proof_ref",
                    reason: "DropAuthorize proof action requires a proof id".to_string(),
                })?;
                args.assert_no_args("Proof.DropAuthorize")?;

                self.tracker.write_with(|state| {
                    // Giving up an authorization only shrinks this frame's own auth scope, so it succeeds for any
                    // id: an id the frame never authorized is already in the state being asked for. That makes it
                    // answerless by construction, which is what keeps it from reporting whether a proof is live at
                    // an id the frame does not hold — the ids are a dense counter, so an answer would enumerate
                    // every proof in the transaction. `ProofAccess::drop` is the only route here, and a `Drop` has
                    // nowhere to report a failure, so a rejection would abort the transaction from a drop point
                    // the template author never wrote.
                    state.current_call_scope_mut()?.auth_scope_mut().remove_proof(&proof_id);

                    Ok(InvokeResult::unit())
                })
            },
            ProofAction::Drop => {
                let proof_id = proof_ref.proof_id().ok_or_else(|| RuntimeError::InvalidArgument {
                    argument: "proof_ref",
                    reason: "Drop proof action requires a proof id".to_string(),
                })?;
                args.assert_no_args("Proof.Drop")?;

                self.tracker.write_with(|state| state.drop_proof(proof_id))?;

                Ok(InvokeResult::unit())
            },
        }
    }

    fn workspace_invoke(&self, action: WorkspaceAction, args: EngineArgs) -> Result<InvokeResult, RuntimeError> {
        self.invoke_modules_on_runtime_call("workspace_invoke")?;

        debug!(target: LOG_TARGET, "Workspace invoke: {:?}", action,);

        match action {
            // Names an output on the workspace so that you can refer to it as an
            // Arg::Variable
            WorkspaceAction::PutLastInstructionOutput => {
                let key = args.assert_one_arg()?;
                let last_output = self
                    .tracker
                    .take_last_instruction_output()
                    .ok_or(RuntimeError::NoLastInstructionOutput)?;

                self.validate_return_value(&last_output)?;

                self.tracker
                    .with_workspace_mut(|workspace| workspace.insert(key, last_output))?;
                Ok(InvokeResult::unit())
            },
            WorkspaceAction::Get => {
                let id: WorkspaceOffsetId = args.assert_one_arg()?;
                self.tracker.read_with(|state| {
                    let value =
                        state
                            .workspace()
                            .get(id)?
                            .cloned()
                            .ok_or_else(|| RuntimeError::ItemNotOnWorkspace {
                                id,
                                existing_ids: state.workspace().all_ids_iter().collect(),
                            })?;
                    Ok(InvokeResult::from_value(value)?)
                })
            },

            WorkspaceAction::DropAllProofs => {
                args.assert_no_args("WorkspaceAction::DropAllProofs")?;
                let proofs = self.tracker.with_workspace_mut(|workspace| workspace.take_all_proofs());

                self.tracker.write_with(|state| {
                    for proof_id in proofs {
                        state.drop_proof(proof_id)?;
                    }
                    Ok(InvokeResult::unit())
                })
            },
            WorkspaceAction::Assert => {
                args.assert_n_args(2)?;
                let key: WorkspaceOffsetId = args.get(0)?;
                let assertion: Assertion = args.get(1)?;

                self.tracker.read_with(|state| {
                    state.workspace_assert(key, assertion)?;
                    Ok(InvokeResult::unit())
                })
            },
            WorkspaceAction::DropAll => {
                args.assert_no_args("WorkspaceAction::DropAll")?;
                let proofs = self.tracker.with_workspace_mut(|workspace| {
                    workspace.clear_items();
                    workspace.take_all_proofs()
                });

                self.tracker.write_with(|state| {
                    for proof_id in proofs {
                        state.drop_proof(proof_id)?;
                    }
                    Ok(InvokeResult::unit())
                })
            },
            WorkspaceAction::AuthorizeProof => {
                let key: WorkspaceOffsetId = args.assert_one_arg()?;
                self.tracker.write_with(|state| {
                    let proof_id = state.workspace_proof(key)?;
                    let scope = state.current_call_scope_mut()?;
                    scope.require_held_proof(&proof_id)?;
                    if scope.auth_scope().contains_proof(&proof_id) {
                        return Err(RuntimeError::ProofAlreadyAuthorized { proof_id });
                    }
                    scope.auth_scope_mut().add_proof(proof_id);
                    Ok(InvokeResult::unit())
                })
            },
            WorkspaceAction::DropProofAuthorization => {
                let key: WorkspaceOffsetId = args.assert_one_arg()?;
                self.tracker.write_with(|state| {
                    let proof_id = state.workspace_proof(key)?;
                    if !state.current_call_scope_mut()?.auth_scope_mut().remove_proof(&proof_id) {
                        return Err(RuntimeError::ProofNotAuthorized { proof_id });
                    }
                    Ok(InvokeResult::unit())
                })
            },
        }
    }

    fn non_fungible_invoke(
        &self,
        nf_addr: NonFungibleAddress,
        action: NonFungibleAction,
        args: EngineArgs,
    ) -> Result<InvokeResult, RuntimeError> {
        self.invoke_modules_on_runtime_call("non_fungible_invoke")?;
        debug!(
            target: LOG_TARGET,
            "NonFungible invoke: {} {:?}",
            nf_addr,
            action,
        );

        match action {
            NonFungibleAction::GetData => {
                args.assert_no_args("NonFungibleAction::GetData")?;
                self.tracker.write_with(|state| {
                    let nft_lock = state.read_lock_substate(SubstateId::NonFungible(nf_addr.clone()))?;
                    let nft = state.get_non_fungible(&nft_lock)?;
                    let contents = nft
                        .contents()
                        .ok_or_else(|| RuntimeError::InvalidOpNonFungibleBurnt {
                            op: "GetData",
                            resource_address: *nf_addr.resource_address(),
                            nf_id: nf_addr.id().clone(),
                        })?
                        .data()
                        .clone();
                    state.unlock_substate(nft_lock)?;
                    Ok(InvokeResult::from_value(contents)?)
                })
            },
            NonFungibleAction::GetMutableData => {
                args.assert_no_args("NonFungibleAction::GetMutableData")?;

                self.tracker.write_with(|state| {
                    let nft_lock = state.read_lock_substate(SubstateId::NonFungible(nf_addr.clone()))?;
                    let nft = state.get_non_fungible(&nft_lock)?;
                    let contents = nft
                        .contents()
                        .ok_or_else(|| RuntimeError::InvalidOpNonFungibleBurnt {
                            op: "GetMutableData",
                            resource_address: *nf_addr.resource_address(),
                            nf_id: nf_addr.id().clone(),
                        })?
                        .mutable_data()
                        .clone();
                    state.unlock_substate(nft_lock)?;

                    Ok(InvokeResult::from_value(contents)?)
                })
            },
        }
    }

    fn consensus_invoke(&self, action: ConsensusAction) -> Result<InvokeResult, RuntimeError> {
        self.invoke_modules_on_runtime_call("consensus_invoke")?;
        match action {
            ConsensusAction::GetCurrentEpoch => {
                let epoch = self.tracker.get_current_epoch()?;
                Ok(InvokeResult::encode(&epoch)?)
            },
            ConsensusAction::GetCurrentEpochHash => {
                let hash = self.tracker.get_current_epoch_hash()?;
                Ok(InvokeResult::encode(&hash)?)
            },
        }
    }

    fn generate_random_invoke(&self, action: GenerateRandomAction) -> Result<InvokeResult, RuntimeError> {
        self.invoke_modules_on_runtime_call("generate_random_invoke")?;
        match action {
            GenerateRandomAction::GetRandomBytes { len } => {
                let len = len as usize;
                if len > limits::ENGINE_LIMITS.max_random_bytes_len {
                    return Err(LimitError::MaxRandomBytesLenExceeded { len }.into());
                }
                let random = self.tracker.get_pseudorandom_bytes(len)?;
                Ok(InvokeResult::encode(&random)?)
            },
        }
    }

    fn generate_uuid(&self) -> Result<[u8; 32], RuntimeError> {
        self.invoke_modules_on_runtime_call("generate_uuid")?;
        self.tracker.write_with(|state| {
            let epoch_hash = state.get_current_epoch_hash()?;
            Ok(state.id_provider()?.new_uuid(&epoch_hash)?)
        })
    }

    fn complete_instruction(&self, output: Option<IndexedValue>) -> Result<(), RuntimeError> {
        if output.is_some() {
            self.invoke_modules_on_runtime_call("set_last_instruction_output")?;
        }
        for module in self.modules.iter() {
            module.on_instruction_output(&self.tracker, output.as_ref())?;
        }
        if let Some(output) = output {
            self.tracker.write_with(|state| {
                state.set_last_instruction_output(output);
            });
        }
        Ok(())
    }

    fn claim_burn(&self, claim: MinotariBurnClaimProof) -> Result<BucketId, RuntimeError> {
        let epoch = self.tracker.get_current_epoch()?;
        self.tracker
            .charge_native_execution(tari_engine_types::limits::NativeExecutionPoints::PER_CLAIM_BURN)?;
        let verified = self
            .claim_burn_proof_verifier
            .verify_claim_proof(epoch, &claim)
            .map_err(|e| {
                warn!(target: LOG_TARGET, "Claim burn failed - proof verification failed: {}", e);
                match e {
                    ClaimProofError::Rejected(ClaimProofRejection::Invalid(details)) => {
                        RuntimeError::InvalidClaimProof { details }
                    },
                    ClaimProofError::Rejected(ClaimProofRejection::NotYetValid(details)) => {
                        RuntimeError::ClaimProofNotYetValid { details }
                    },
                    ClaimProofError::VerifierFault(details) => RuntimeError::ClaimProofVerifierFault { details },
                }
            })?;
        // The burner also knows the commitment opening and can make a valid ownership proof, so only the claim key's
        // signature shows the claimant is the recipient the burn output names
        let claim_public_key = verified.claim_public_key;
        self.tracker.read_with(|state| {
            let badge = NonFungibleAddress::from_public_key(claim_public_key);
            if !state.base_call_scope().auth_scope().contains_badge(&badge) {
                return Err(RuntimeError::ClaimNotSignedByClaimKey { claim_public_key });
            }
            Ok(())
        })?;

        self.tracker.write_with(|state_mut| {
            // The burner knows the burn commitment's opening, so a stealth transfer of theirs can create, and spend, a
            // UTXO addressed by that commitment. The tombstone is therefore the only substate a claim addresses by the
            // commitment: only a claim creates one, and a claim needs the claim key's signature.
            let address = ClaimedOutputTombstoneAddress::from_commitment(claim.commitment);
            state_mut.new_substate(address, ClaimedOutputTombstone { value: claim.value })?;

            // The claimed value is revealed in a bucket that the claim transaction moves on. The ownership proof the
            // verifier checked binds `claim.value` to the commitment.
            //
            // This mints value with no `increase_total_supply` counterpart, which is sound only because the genesis
            // TARI resource both disables supply tracking and denies `Burn` (see `get_stealth_tari_resource`).
            // Enabling either would need an increase here to balance the decrease that
            // `ResourceAction::StealthUtxoBurn` applies when TARI is burnt.
            let bucket_id = state_mut.new_bucket_id()?;
            state_mut.new_bucket(
                bucket_id,
                ResourceContainer::stealth(TARI_TOKEN, Amount::from(claim.value)),
            )?;
            Ok(bucket_id)
        })
    }

    fn claim_validator_fees(
        &self,
        pool_address: ValidatorFeePoolAddress,
        max_amount: Option<Amount>,
    ) -> Result<BucketId, RuntimeError> {
        self.tracker.write_with_authorization(|state| {
            let resource = match max_amount {
                Some(max_amount) => state.withdraw_fees_from_pool_up_to(pool_address, max_amount)?,
                None => state.withdraw_all_fees_from_pool(pool_address)?,
            };
            let bucket_id = state.new_bucket_id()?;
            state.new_bucket(bucket_id, resource)?;
            Ok(bucket_id)
        })
    }

    fn metered_fee_receipt(&self) -> FeeReceipt {
        FeeReceipt::builder()
            .with_cost_breakdown(self.tracker.fee_charges())
            .build()
    }

    fn required_fee_payment(&self) -> u64 {
        self.tracker.required_fee_payment()
    }

    fn created_substates(&self) -> Result<Vec<SubstateId>, RuntimeError> {
        self.tracker.created_substate_ids()
    }

    fn checkpoint_fee_intent(&self) -> Result<(), RuntimeError> {
        // Price the state the fee intent ended on before testing what was paid against it. This is
        // the state a transaction that cannot afford its main intent falls back to committing, so a
        // payment that cannot cover it cannot commit anything at all — better established here,
        // before the main instructions run, than after they have consumed compute nobody pays for.
        self.invoke_modules_on_fee_checkpoint()?;
        if !self.tracker.is_fee_state_dry_run() && self.tracker.total_fee_payments() < self.tracker.total_fee_charges()
        {
            return Err(RuntimeError::InsufficientFeesPaid {
                required_fee: self.tracker.required_fee_payment(),
                fees_paid: self.tracker.total_fee_payments(),
            });
        }
        self.tracker.fee_checkpoint()
    }

    fn finalize(&self) -> Result<FinalizeResult, RuntimeError> {
        // Finalization adds no fee charge of its own. The template never invoked it, and the compute
        // allowance is sized against the charges standing when it is computed, so anything charged
        // afterwards puts the total past what that allowance was sized to fit inside.
        self.finalize_with(None, false).map(|(result, _)| result)
    }

    fn finalize_with_fee_intent_fallback(&self) -> Result<(FinalizeResult, Option<FeeIntentFallback>), RuntimeError> {
        self.finalize_with(None, true)
    }

    fn finalize_failure(&self, reason: RejectReason) -> Result<FinalizeResult, RuntimeError> {
        self.finalize_with(Some(reason), false).map(|(result, _)| result)
    }

    fn validate_finalized(&self) -> Result<(), RuntimeError> {
        self.tracker.read_with(|state| {
            state.validate_finalized()?;
            Ok(())
        })
    }

    fn caller_context_invoke(
        &self,
        action: CallerContextAction,
        args: EngineArgs,
    ) -> Result<InvokeResult, RuntimeError> {
        self.invoke_modules_on_runtime_call("caller_context_invoke")?;

        match action {
            CallerContextAction::GetCallerPublicKey => {
                args.assert_no_args("CallerContextAction::GetCallerPublicKey")?;
                Ok(InvokeResult::encode(
                    &self.main_signer("CallerContext::transaction_signer_public_key")?,
                )?)
            },
            CallerContextAction::GetComponentAddress => self.tracker.read_with(|state| {
                args.assert_no_args("CallerContextAction::GetComponentAddress")?;
                let call_scope = state.current_call_scope()?;
                let maybe_address = call_scope
                    .get_current_component_lock()
                    .map(|l| l.substate_id().as_component_address().unwrap());
                Ok(InvokeResult::encode(&maybe_address)?)
            }),
            CallerContextAction::AssertBadgeInScope => {
                let badge: NonFungibleAddress = args.assert_one_arg()?;
                let rule = AccessRule::Restricted(RestrictedAccessRule::Require(RequireRule::Require(
                    RuleRequirement::NonFungibleAddress(badge.clone()),
                )));
                self.tracker.write_with_authorization(|state| {
                    if !state.authorization().check_access_rule(&rule)? {
                        return Err(RuntimeError::BadgeNotInScope { badge });
                    }
                    Ok(InvokeResult::unit())
                })
            },
        }
    }

    fn allocate_address_invoke(&self, action: AddressAllocationInvokeArg) -> Result<InvokeResult, RuntimeError> {
        self.invoke_modules_on_runtime_call("allocate_address_invoke")?;

        self.tracker.write_with(|state| {
            match action {
                AddressAllocationInvokeArg::GetAddress(id) => {
                    let allocation = state.get_allocated_address(id)?;
                    match allocation.substate_id() {
                        SubstateId::Component(addr) => Ok(InvokeResult::encode(&addr)?),
                        SubstateId::Resource(addr) => Ok(InvokeResult::encode(&addr)?),
                        // Engine creates the allocations, so never creates other unsupported variants
                        _ => unreachable!("Invalid SubstateId variant. Allocation created for unsupported substate"),
                    }
                },
                AddressAllocationInvokeArg::CreateComponentAllocation { public_key } => {
                    // Validate the public key
                    let _ignore = public_key
                        .map(|pk| {
                            RistrettoPublicKey::from_canonical_bytes(pk.as_bytes()).map_err(|_| {
                                RuntimeError::InvalidArgument {
                                    argument: "public_key",
                                    reason: "Invalid RistrettoPublicKeyBytes".to_string(),
                                }
                            })
                        })
                        .transpose()?;

                    let template = *state.current_template()?;
                    let mut id_provider = state.id_provider()?;
                    let address = public_key
                        .as_ref()
                        .map(|public_key| id_provider.derive_new_component_address(&template, public_key))
                        .unwrap_or_else(|| id_provider.new_component_address())?;

                    if state.substate_exists(&SubstateId::Component(address))? {
                        return Err(RuntimeError::ComponentAlreadyExists { address });
                    }

                    let id = state.new_address_allocation(address)?;
                    Ok(InvokeResult::encode(&ComponentAddressAllocation::new(id))?)
                },
                AddressAllocationInvokeArg::CreateResourceAllocation => {
                    let address = state.id_provider()?.new_resource_address()?;
                    let id = state.new_address_allocation(address)?;
                    Ok(InvokeResult::encode(&ResourceAddressAllocation::new(id))?)
                },
            }
        })
    }

    fn call_invoke(&self, action: CallAction, args: EngineArgs) -> Result<InvokeResult, RuntimeError> {
        self.invoke_modules_on_runtime_call("call_invoke")?;
        self.tracker.read_with(|state| {
            let frame = state.current_call_frame()?;
            if !frame.is_cross_template_calls_allowed() {
                return Err(RuntimeError::CrossTemplateCallNotAllowed { action });
            }
            Ok(())
        })?;

        debug!(
            target: LOG_TARGET,
            "Call invoke: {:?} {:?}",
            action,
            args,
        );

        let exec_result = match action {
            CallAction::CallFunction => {
                let CallFunctionArg {
                    template_address,
                    function,
                    args,
                } = args.assert_one_arg()?;

                self.invoke_template_function(
                    &template_address,
                    &function,
                    args.into_iter().map(InstructionArg::Literal).collect(),
                    None,
                )?
            },
            CallAction::CallMethod => {
                let CallMethodArg {
                    component_address,
                    method,
                    args,
                } = args.assert_one_arg()?;

                self.invoke_component_method(component_address, &method, args, None, None)?
            },
        };

        Ok(InvokeResult::from_value(exec_result.into_value())?)
    }

    fn builtin_template_invoke(&self, action: BuiltinTemplateAction) -> Result<InvokeResult, RuntimeError> {
        self.invoke_modules_on_runtime_call("builtin_template_invoke")?;

        let address = match action {
            BuiltinTemplateAction::GetTemplateAddress { builtin: bultin } => match bultin {
                BuiltinTemplate::Account => ACCOUNT_TEMPLATE_ADDRESS,
                BuiltinTemplate::AccountNft => NFT_FAUCET_TEMPLATE_ADDRESS,
            },
        };

        Ok(InvokeResult::encode(&address)?)
    }

    fn check_component_access_rules(&self, method: &str) -> Result<(), RuntimeError> {
        self.tracker
            .write_with_authorization(|state| state.authorization().check_current_component_access_rules(method))
    }

    fn check_signer_badge_in_scope(&self, public_key: RistrettoPublicKeyBytes) -> Result<(), RuntimeError> {
        self.tracker.read_with(|state| {
            let badge = NonFungibleAddress::from_public_key(public_key);
            if !state.base_call_scope().auth_scope().contains_badge(&badge) {
                return Err(RuntimeError::SignerBadgeNotInScope { public_key });
            }
            Ok(())
        })
    }

    fn check_component_upgrade_rule(&self, component_lock: &LockedSubstate) -> Result<(), RuntimeError> {
        self.tracker.write_with_authorization(|state| {
            let component = state.get_component(component_lock)?;
            let action = NativeAction::UpdateComponentTemplate;
            match component.upgrade_rule() {
                UpdateRule::Locked => Err(RuntimeError::ComponentUpgradesLocked {
                    component: component_lock.substate_id().as_component_address().ok_or_else(|| {
                        RuntimeError::InvariantError {
                            function: "check_component_upgrade_rule",
                            details: "Locked substate is not a component".to_string(),
                        }
                    })?,
                }),
                UpdateRule::Owner => state
                    .authorization()
                    .require_ownership(action, component.as_ownership()),
                UpdateRule::AccessRule(rule) => {
                    if state.authorization().check_access_rule(rule)? {
                        Ok(())
                    } else {
                        Err(RuntimeError::AccessDenied {
                            action_ident: action.into(),
                        })
                    }
                },
            }
        })
    }

    fn update_component_template(&self, new_template: TemplateAddress) -> Result<(), RuntimeError> {
        let access_rules = self.tracker.read_with(|state| {
            let locked = state
                .current_call_scope()?
                .get_current_component_lock()
                .ok_or_else(|| RuntimeError::InvariantError {
                    function: "update_component_template",
                    details: "No current component lock in call scope".to_string(),
                })?;
            Ok::<_, RuntimeError>(state.get_component(locked)?.access_rules().clone())
        })?;
        // A method rule that the new template lacks would be lost silently, and a later template that re-adds the
        // method would put it under the component's default rule. The developer keeps the rule by leaving a stub
        // method in the new template, or by setting the access rules under the current template before upgrading.
        if access_rules.num_access_rules() > 0 {
            let template_def = self.get_template_def(&new_template)?;
            validate_component_access_rule_methods(&access_rules, &template_def)?;
        }

        self.tracker.write_with(|state_mut| {
            let locked = state_mut
                .current_call_scope()?
                .get_current_component_lock()
                .cloned()
                .ok_or_else(|| RuntimeError::InvariantError {
                    function: "update_component_template",
                    details: "No current component lock in call scope".to_string(),
                })?;

            let prev_template = state_mut.set_component_template_address(&locked, new_template)?;

            state_mut.push_event(Event::std(
                Some(locked.substate_id().clone()),
                new_template,
                "component",
                "template_update",
                try_metadata!("prev_template" => prev_template)?,
            ))?;
            Ok(())
        })
    }

    fn validate_return_value(&self, value: &IndexedValue) -> Result<(), RuntimeError> {
        self.tracker
            .read_with(|state| state.check_all_substates_known(value.well_known_types()))
    }

    fn push_call_frame(&self, frame: PushCallFrame, restrict_to: Option<FrameWriteMode>) -> Result<(), RuntimeError> {
        self.tracker.push_call_frame(frame, restrict_to)
    }

    fn pop_call_frame(&self, returned: &IndexedWellKnownTypes) -> Result<(), RuntimeError> {
        self.tracker.pop_call_frame(returned)?;
        Ok(())
    }

    fn publish_template(
        &self,
        template: TemplateBlob,
        metadata_hash: Option<TemplateMetadataHash>,
        template_def: TemplateDef,
    ) -> Result<(), RuntimeError> {
        self.invoke_modules_on_runtime_call("publish_template")?;
        self.tracker.write_with(|state_mut| {
            let template_byte_size = template.len();
            let code_hash = hash_template_code(&template);
            let author = self.main_signer("publishing a template")?;
            let template_address = PublishedTemplateAddress::from_author_and_binary_hash(&author, &code_hash);
            let epoch = state_mut.get_current_epoch()?;
            let template_name = template_def
                .template_name()
                .try_into()
                .map_err(|_| RuntimeError::InvalidArgument {
                    argument: "template_name",
                    reason: format!(
                        "Template name exceeds maximum length of {} bytes",
                        limits::ENGINE_LIMITS.max_template_name_length
                    ),
                })?;
            state_mut.new_substate(
                template_address,
                SubstateValue::Template(PublishedTemplate {
                    template_name,
                    binary: template,
                    author,
                    at_epoch: epoch.as_u64(),
                    metadata_hash: metadata_hash.clone(),
                }),
            )?;
            // Mark template substate as owned by current call stack
            let scope_mut = state_mut.current_call_scope_mut()?;
            scope_mut.move_node_to_owned(&template_address.into())?;
            // Publish template event
            let mut metadata = Metadata::new();
            metadata.try_insert("template_byte_size", &template_byte_size)?;
            state_mut.push_event(Event::std(
                Some(template_address.into()),
                template_address.as_hash(),
                "template",
                "publish",
                metadata,
            ))?;

            Ok(())
        })
    }

    fn put_on_workspace(&self, id: WorkspaceId, value: IndexedValue) -> Result<(), RuntimeError> {
        self.invoke_modules_on_runtime_call("put_on_workspace")?;

        self.validate_return_value(&value)?;

        self.tracker
            .with_workspace_mut(|workspace| workspace.insert(id, value))?;
        Ok(())
    }

    fn intrinsic_invoke(&self, intrinsic: IntrinsicId, args: EngineArgs) -> Result<InvokeResult, RuntimeError> {
        self.invoke_modules_on_runtime_call("intrinsic_invoke")?;

        // Priced from the declared arguments and charged before the work runs, so a transaction that
        // cannot afford an intrinsic traps without the validator having performed it.
        let points = intrinsics::price(intrinsic, &args)?;
        self.tracker.charge_native_execution(points)?;

        intrinsics::dispatch(intrinsic, args, &*self.tracker)
    }

    fn spend_context_invoke(&self, action: SpendContextAction) -> Result<InvokeResult, RuntimeError> {
        self.invoke_modules_on_runtime_call("spend_context_invoke")?;

        // Only reachable while a spend-script predicate is executing; `spend_exec_context` is set immediately before
        // the predicate is invoked and cleared immediately after.
        let ctx = self.spend_exec_context.borrow();
        let ctx = ctx.as_ref().ok_or(RuntimeError::SpendContextUnavailable)?;

        match action {
            SpendContextAction::Inputs => Ok(InvokeResult::encode(&ctx.statement.inputs)?),
            SpendContextAction::Outputs => Ok(InvokeResult::encode(&ctx.statement.outputs)?),
            SpendContextAction::CurrentInput => Ok(InvokeResult::encode(&CurrentInputView {
                index: ctx.current_input_index,
                commitment: ctx.current_input_commitment,
                condition_root: Some(ctx.current_input_condition_root),
            })?),
            SpendContextAction::RevealedInputAmount => Ok(InvokeResult::encode(&ctx.statement.revealed_input_amount)?),
            SpendContextAction::RevealedOutputAmount => {
                Ok(InvokeResult::encode(&ctx.statement.revealed_output_amount)?)
            },
            SpendContextAction::AssertCovenantBalanced { max_revealed } => {
                let balanced =
                    ctx.statement
                        .covenant_balanced(&ctx.current_input_condition_root, max_revealed, |points| {
                            self.tracker.charge_native_execution(points)
                        })?;
                Ok(InvokeResult::encode(&balanced)?)
            },
            SpendContextAction::WitnessData => Ok(InvokeResult::encode(&ctx.witness_data)?),
        }
    }

    /// Create a new address allocation for the provided substate type and entity id
    fn allocate_address(
        &self,
        substate_type: AllocatableAddressType,
        entity_id: EntityId,
        workspace_id: WorkspaceId,
    ) -> Result<AllocateAddressResult, RuntimeError> {
        self.tracker.write_with(|state| match substate_type {
            AllocatableAddressType::Component => {
                let address = state.id_provider_for_entity(entity_id).new_component_address()?;
                let id = state.new_address_allocation(address)?;
                let value = IndexedValue::from_type(&ComponentAddressAllocation::new(id))?;
                state.workspace_mut().insert(workspace_id, value)?;
                Ok(AllocateAddressResult::ComponentAddress(
                    ComponentAddressAllocation::new(id),
                ))
            },
            AllocatableAddressType::Resource => {
                let address = state.id_provider_for_entity(entity_id).new_resource_address()?;
                let id = state.new_address_allocation(address)?;
                let value = IndexedValue::from_type(&ResourceAddressAllocation::new(id))?;
                state.workspace_mut().insert(workspace_id, value)?;
                Ok(AllocateAddressResult::ResourceAddress(ResourceAddressAllocation::new(
                    id,
                )))
            },
        })
    }

    fn stealth_transfer(
        &self,
        resource_address: ResourceAddressRef,
        statement: StealthTransferStatement,
        revealed_funds_bucket_id: Option<BucketId>,
    ) -> Result<Option<BucketId>, RuntimeError> {
        // (T1) Creation time: a stealth output commits only an opaque `condition_root`, so its leaves are hidden and
        // cannot be validated at creation — that happens at spend time when a leaf is revealed (below / inline). The
        // only creation-time invariant is that an output is spendable by at least one path (`spend_key` or
        // `condition_root` is `Some`), enforced by `validate_stealth_outputs_statement` during execution.

        let address = self.tracker.read_with(|state| {
            let address = state.resolve_resource_address_ref(resource_address.clone())?;
            state.check_write_allowed(&SubstateId::Resource(address))?;
            Ok::<_, RuntimeError>(address)
        })?;

        // The fee intent runs on free-compute credit, so the transfers it may perform are capped. Checked before any
        // resource read, charge or crypto so an over-cap fee intent is rejected up front. Both the `StealthTransfer`
        // instruction and a template's `ResourceManager::stealth_transfer` reach here, so neither route escapes it.
        self.tracker.admit_fee_intent_stealth_transfer()?;

        // The whole statement's native verification cost is charged against the payment-funded
        // allowance before any of it runs (the authorisation pass below included), so a transaction
        // that will not pay traps here without extracting the crypto work. The resource peek is a
        // cheap substate read that determines whether the viewable-balance surcharge applies.
        let (admitted, has_view_key) = self.tracker.write_with(|state| {
            let admitted = state.admit_stealth_transfer(statement)?;
            let resource_lock = state.read_lock_substate(SubstateId::Resource(address))?;
            let has_view_key = state.get_resource(&resource_lock)?.view_key().is_some();
            state.unlock_substate(resource_lock)?;
            Ok::<_, RuntimeError>((admitted, has_view_key))
        })?;
        self.tracker
            .charge_native_execution(tari_engine_types::stealth::transfer_native_points(
                admitted.statement(),
                has_view_key,
            ))?;

        self.authorize_resource_action(address, ResourceAuthAction::Withdraw, |_| Ok(()))?;

        // (T2) Spend time: authorise every input BEFORE the spend executes, so a rejection leaves the inputs unspent.
        // This is the authoritative, mandatory security gate for all spend paths (key path, AccessRule, and WASM
        // predicate).
        //
        // Covenant soundness invariant: a covenant balance proof binds the partition's output commitments but trusts
        // their values to be in range; `execute_stealth_transfer` below range-proofs those same `statement` outputs.
        // Both run pre-commit in this one atomic call, so the two MUST stay coupled — decoupling them would reopen a
        // wraparound forgery in the covenant check.
        self.verify_input_authorizations(resource_address.clone(), admitted.statement())?;

        self.tracker.write_with_authorization(|state_mut| {
            let Some(container) =
                state_mut.execute_stealth_transfer(resource_address, admitted, revealed_funds_bucket_id)?
            else {
                return Ok(None);
            };
            let bucket_id = state_mut.new_bucket_id()?;
            state_mut.new_bucket(bucket_id, container)?;
            Ok(Some(bucket_id))
        })
    }

    fn pay_fee(&self, pay_fee: PayFee) -> Result<(), RuntimeError> {
        if self.tracker.is_fee_intent_checkpointed() {
            return Err(RuntimeError::FeePaymentInMainIntent);
        }

        self.tracker.write_with(|state_mut| {
            match pay_fee {
                PayFee::FromBucket { bucket } => {
                    let value = state_mut
                        .workspace()
                        .get(bucket)?
                        .ok_or_else(|| RuntimeError::ItemNotOnWorkspace {
                            id: bucket,
                            existing_ids: state_mut.workspace().all_ids_iter().collect(),
                        })?;
                    let input_bucket =
                        tari_bor::from_value::<BucketId>(value).map_err(|e| RuntimeError::InvalidArgument {
                            argument: "bucket",
                            reason: format!("PayFee::FromBucket: Expected workspace ID to contain a BucketId: {e}"),
                        })?;
                    let bucket = state_mut.take_bucket(input_bucket)?;
                    Self::check_bucket_is_unlocked("pay a fee from", input_bucket, &bucket)?;

                    // An empty Tari bucket pays nothing: consume it and record no fee payment.
                    if *bucket.resource_address() == TARI_TOKEN && bucket.unlocked_amount().is_zero() {
                        return Ok(());
                    }

                    // No refunds
                    state_mut.pay_fee(bucket.take_all(), None)?;
                    Ok(())
                },
            }
        })
    }

    fn track_template_loaded(
        &self,
        template_address: &TemplateAddress,
        bytes_loaded: usize,
    ) -> Result<(), RuntimeError> {
        // Built-in templates are zero-cost
        if is_builtin_template_address(template_address) {
            return Ok(());
        }

        // Note: per-transaction dedup is intentionally pushed into the modules that want it (see
        // `FeeModule::on_template_loaded`), so observer-style modules continue to see every load.
        for module in self.modules.iter() {
            module.on_template_loaded(&self.tracker, template_address, bytes_loaded)?;
        }
        Ok(())
    }

    fn record_wasm_execution(&self, points_consumed: u64) -> Result<(), RuntimeError> {
        // Accumulate into the transaction-wide total unconditionally (not via a module) so the
        // per-transaction budget is enforced even when fee charging is disabled. The fee module
        // reads this total in `on_before_finalize` to compute the WASM execution charge.
        self.tracker.accumulate_wasm_points(points_consumed);
        for module in self.modules.iter() {
            module.on_wasm_execution(&self.tracker, points_consumed)?;
        }
        Ok(())
    }

    fn charge_template_instantiation(&self, shape: &ModuleShape) -> Result<(), RuntimeError> {
        self.tracker
            .charge_native_execution(tari_engine_types::limits::instantiation_points(shape))
    }

    fn charge_return_value_read(&self, raw: &[u8]) -> Result<(), EngineArgChargeError> {
        use tari_engine_types::limits::{self, NativeExecutionPoints, return_value_points};

        self.charge_cbor_read(
            raw,
            limits::MAX_CBOR_NESTING_DEPTH,
            NativeExecutionPoints::PER_RETURN_VALUE_ITEM,
            return_value_points,
        )
    }

    fn charge_engine_arg(&self, input: &[u8]) -> Result<(), EngineArgChargeError> {
        use tari_engine_types::limits::{self, NativeExecutionPoints, engine_arg_points};

        self.charge_cbor_read(
            input,
            limits::MAX_CBOR_NESTING_DEPTH,
            NativeExecutionPoints::PER_ENGINE_ARG_ITEM,
            engine_arg_points,
        )
    }

    fn charge_engine_call(&self) -> Result<(), RuntimeError> {
        self.tracker
            .charge_native_execution(tari_engine_types::limits::NativeExecutionPoints::PER_ENGINE_CALL)
    }

    fn charge_engine_response(&self, response: &[u8]) -> Result<(), RuntimeError> {
        use tari_engine_types::limits::{NativeExecutionPoints, engine_response_points};

        // The response is the engine's own encoding of values bounded when they were decoded, wrapped
        // a few levels deeper, so its depth is left unchecked.
        self.charge_cbor_read(
            response,
            usize::MAX,
            NativeExecutionPoints::PER_ENGINE_RESPONSE_ITEM,
            engine_response_points,
        )
        .map_err(|e| match e {
            EngineArgChargeError::Unreadable(e) => RuntimeError::InvariantError {
                function: "charge_engine_response",
                details: format!("the engine encoded a response it cannot read back: {e}"),
            },
            EngineArgChargeError::Runtime(e) => e,
        })
    }

    fn wasm_instances(&self) -> &RefCell<WasmInstanceCache> {
        &self.wasm_instances
    }

    fn charge_template_compile(&self, binary_bytes: u64, counts: &CompileCounts) -> Result<(), RuntimeError> {
        self.tracker
            .charge_native_execution(tari_engine_types::limits::template_compile_points(binary_bytes, counts))
    }

    fn wasm_points_consumed(&self) -> u64 {
        self.tracker.accumulated_wasm_points()
    }

    fn native_points_consumed(&self) -> u64 {
        self.tracker.accumulated_native_points()
    }

    fn compute_allowance(&self) -> Option<ComputeAllowance> {
        self.tracker.compute_allowance()
    }

    fn resolve_args(
        &self,
        prepend: Option<InstructionArg>,
        args: &[InstructionArg],
    ) -> Result<Vec<tari_bor::Value>, RuntimeError> {
        let prepend_len = usize::from(prepend.is_some());
        let total_len = prepend_len + args.len();
        if total_len > limits::WASM_LIMITS.max_function_arguments {
            return Err(ArgumentValidationError::TooManyArguments {
                got: total_len,
                max: limits::WASM_LIMITS.max_function_arguments,
            }
            .into());
        }

        prepend
            .iter()
            .chain(args.iter())
            .map(|arg| match arg {
                InstructionArg::Workspace(id) => self.resolve_workspace_id(id),
                InstructionArg::Literal(v) => self.decode_charged_arg(v),
                // A blob arg's value is the raw bytes of the referenced blob, decoded as CBOR
                // by the same path as `Literal`. Lookup is against the surrounding transaction's
                // `Blobs`, set on the runtime at construction time. A blob is paid for once by
                // transaction weight but may be referenced many times, so each reference is charged.
                InstructionArg::Blob(idx) => {
                    let blob = self
                        .blobs
                        .get(*idx)
                        .ok_or(ArgumentValidationError::BlobIndexOutOfBounds {
                            index: *idx,
                            count: self.blobs.len(),
                        })?;
                    self.decode_charged_arg(blob.as_bytes())
                },
            })
            .collect()
    }

    fn resolve_workspace_id(&self, workspace_id: &WorkspaceOffsetId) -> Result<tari_bor::Value, RuntimeError> {
        use tari_engine_types::limits::{NativeExecutionPoints, engine_arg_points};

        let id = *workspace_id;
        let affordable = self.tracker.native_execution_headroom() / NativeExecutionPoints::PER_ENGINE_ARG_ITEM;
        let counted = self.tracker.with_workspace(|workspace| {
            let value = workspace.get(id)?.ok_or_else(|| RuntimeError::ItemNotOnWorkspace {
                id,
                existing_ids: workspace.all_ids_iter().collect(),
            })?;
            Ok::<_, RuntimeError>(
                value
                    .count_items_bounded(affordable, |_| false)
                    .map(|count| (tari_bor::encoded_len(value) as u64, count.items)),
            )
        })?;
        match counted {
            Some((bytes, items)) => {
                self.tracker.charge_native_execution(engine_arg_points(0, items))?;
                self.tracker.charge_native_execution(engine_arg_points(bytes, 0))?;
            },
            None => {
                self.tracker.charge_native_execution(engine_arg_points(0, affordable))?;
                self.tracker.charge_native_execution(engine_arg_points(0, 1))?;
                return Err(RuntimeError::InvariantError {
                    function: "resolve_workspace_id",
                    details: "an item beyond the native execution headroom was affordable".to_string(),
                });
            },
        }
        self.tracker.with_workspace(|workspace| {
            workspace
                .get(id)?
                .cloned()
                .ok_or_else(|| RuntimeError::ItemNotOnWorkspace {
                    id,
                    existing_ids: workspace.all_ids_iter().collect(),
                })
        })
    }
}

fn reject_invalid_rule(argument: &'static str, invalid: Option<InvalidAccessRule>) -> Result<(), RuntimeError> {
    match invalid {
        Some(InvalidAccessRule::EmptyAnyOf) => Err(RuntimeError::EmptyAccessRuleComposite {
            argument,
            composite: "any_of",
        }),
        Some(InvalidAccessRule::EmptyAllOf) => Err(RuntimeError::EmptyAccessRuleComposite {
            argument,
            composite: "all_of",
        }),
        Some(InvalidAccessRule::ThresholdOutOfRange {
            threshold,
            num_requirements,
        }) => Err(RuntimeError::InvalidMOfNThreshold {
            argument,
            threshold,
            num_requirements,
        }),
        Some(InvalidAccessRule::DuplicateRequirement { requirement }) => {
            Err(RuntimeError::DuplicateMOfNRequirement { argument, requirement })
        },
        None => Ok(()),
    }
}

fn find_invalid_owner_rule(owner_rule: &OwnerRule) -> Option<InvalidAccessRule> {
    match owner_rule {
        OwnerRule::ByAccessRule(rule) => rule.find_invalid_rule(),
        OwnerRule::OwnedBySigner | OwnerRule::None | OwnerRule::ByPublicKey(_) => None,
    }
}

fn validate_component_access_rule_methods(
    access_rules: &ComponentAccessRules,
    template_def: &TemplateDef,
) -> Result<(), RuntimeError> {
    for (name, _) in access_rules.method_access_rules_iter() {
        if template_def.functions().iter().all(|f| f.name != *name) {
            return Err(RuntimeError::InvalidMethodAccessRule {
                template_name: template_def.template_name().to_string(),
                details: format!("No method '{}' found in template", name),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use ootle_network::Network;
    use tari_engine_types::{
        confidential::MinotariBurnClaimProof,
        fees::{ExhaustBurnRate, FeeScale},
        limits::engine_arg_points,
        virtual_substate::VirtualSubstates,
    };
    use tari_ootle_common_types::Epoch;
    use tari_ootle_transaction::{Blob, Blobs, TransactionWeight};
    use tari_template_lib::types::{access_rules::ResourceAccessRules, confidential::ConfidentialWithdrawProof};

    use super::*;
    use crate::{
        fees::{FeeTable, WasmMeteringRate},
        runtime::{DeclaredInputLoad, scope::CallScope},
        state_store::memory::MemoryStateStore,
        traits::VerifiedClaim,
    };

    #[derive(Default, Clone)]
    struct DummyTemplateProvider;

    impl TemplateProvider for DummyTemplateProvider {
        type Error = std::convert::Infallible;
        type Template = LoadedTemplate;

        fn get_template(&self, id: &TemplateAddress) -> Result<Option<Self::Template>, Self::Error> {
            Ok(tari_template_builtin::try_get_template_builtin(id)
                .map(|code| crate::wasm::WasmModule::load_template_from_code(code).unwrap()))
        }
    }

    struct DummyClaimVerifier;

    impl ClaimProofVerifier for DummyClaimVerifier {
        fn verify_claim_proof(
            &self,
            _epoch: Epoch,
            _claim: &MinotariBurnClaimProof,
        ) -> Result<VerifiedClaim, ClaimProofError> {
            unimplemented!("not used in blob metering tests")
        }
    }

    fn runtime_with_blobs(blobs: Blobs) -> RuntimeInterfaceImpl<MemoryStateStore, DummyTemplateProvider> {
        let mut fee_table = FeeTable::zero_rated();
        fee_table.per_wasm_point_cost = 1;
        let tracker = StateTracker::new(
            MemoryStateStore::new(),
            VirtualSubstates::empty(),
            CallScope::new(),
            HashSet::new(),
            Hash32::zero(),
            Hash32::zero(),
            TransactionWeight::new(0),
            DeclaredInputLoad::default(),
            WasmMeteringRate::from_fee_table(&fee_table),
            ExhaustBurnRate::default(),
            FeeScale::default(),
            Network::LocalNet,
            false,
        );
        RuntimeInterfaceImpl::initialize(
            tracker,
            Arc::new(DummyTemplateProvider),
            None,
            EntityIdProvider::new(Hash32::zero(), 64),
            Arc::from(Vec::new()),
            Arc::new(DummyClaimVerifier),
            Rc::new(blobs),
        )
        .unwrap()
    }

    #[test]
    fn resolving_a_literal_arg_charges_native_points_for_its_items() {
        let payload: Vec<u64> = (0..128).collect();
        let encoded = tari_bor::encode(&payload).unwrap();
        let items = tari_bor::count_data_items_bounded(&encoded, limits::MAX_CBOR_NESTING_DEPTH, u64::MAX)
            .unwrap()
            .unwrap();
        let expected = engine_arg_points(encoded.len() as u64, items);

        let runtime = runtime_with_blobs(Blobs::default());
        let resolved = runtime
            .resolve_args(None, &[InstructionArg::Literal(encoded.into())])
            .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(runtime.native_points_consumed(), expected);
    }

    #[test]
    fn resolving_a_blob_arg_charges_native_points_per_reference() {
        let payload: Vec<u64> = (0..128).collect();
        let encoded = tari_bor::encode(&payload).unwrap();
        let items = tari_bor::count_data_items_bounded(&encoded, limits::MAX_CBOR_NESTING_DEPTH, u64::MAX)
            .unwrap()
            .unwrap();
        let expected_per_ref = engine_arg_points(encoded.len() as u64, items);
        assert!(expected_per_ref > 0);

        let blobs = Blobs::from_vec(vec![Blob::from(encoded)]);
        let runtime = runtime_with_blobs(blobs);

        assert_eq!(runtime.native_points_consumed(), 0);

        // First reference to blob 0 charges `expected_per_ref`.
        let resolved = runtime.resolve_args(None, &[InstructionArg::Blob(0)]).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(runtime.native_points_consumed(), expected_per_ref);

        // Referencing the same blob three more times charges in direct proportion to the number of references.
        let resolved_multi = runtime
            .resolve_args(None, &[
                InstructionArg::Blob(0),
                InstructionArg::Blob(0),
                InstructionArg::Blob(0),
            ])
            .unwrap();
        assert_eq!(resolved_multi.len(), 3);
        assert_eq!(runtime.native_points_consumed(), expected_per_ref * 4);
    }

    #[test]
    fn repeating_a_large_blob_arg_across_instructions_exhausts_compute_allowance() {
        // Build a CBOR blob with 2,000 items so each reference costs a substantial chunk of compute points,
        // and repeating it across many instructions exceeds `FREE_COMPUTE_GRACE_POINTS`.
        let payload: Vec<u64> = (0..2_000).collect();
        let encoded = tari_bor::encode(&payload).unwrap();
        let blobs = Blobs::from_vec(vec![Blob::from(encoded)]);
        let runtime = runtime_with_blobs(blobs);

        let mut exhausted = false;
        for _ in 0..500 {
            match runtime.resolve_args(None, &[InstructionArg::Blob(0)]) {
                Ok(_) => {},
                Err(RuntimeError::FeeIntentComputeExceeded { .. }) => {
                    exhausted = true;
                    break;
                },
                Err(other) => panic!("expected FeeIntentComputeExceeded, got {other:?}"),
            }
        }
        assert!(
            exhausted,
            "repeatedly referencing a large blob must exhaust the compute allowance"
        );
    }

    #[test]
    fn resolving_a_workspace_arg_charges_native_points_per_reference() {
        let payload: Vec<u64> = (0..128).collect();
        let encoded = tari_bor::encode(&payload).unwrap();
        let items = tari_bor::count_data_items_bounded(&encoded, limits::MAX_CBOR_NESTING_DEPTH, u64::MAX)
            .unwrap()
            .unwrap();
        let expected_per_ref = engine_arg_points(encoded.len() as u64, items);
        assert!(expected_per_ref > 0);

        let runtime = runtime_with_blobs(Blobs::default());
        runtime
            .put_on_workspace(0, IndexedValue::from_type(&payload).unwrap())
            .unwrap();
        assert_eq!(runtime.native_points_consumed(), 0);

        let ws_arg = InstructionArg::Workspace(WorkspaceOffsetId::new(0));
        let resolved = runtime.resolve_args(None, std::slice::from_ref(&ws_arg)).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(runtime.native_points_consumed(), expected_per_ref);

        let resolved_multi = runtime
            .resolve_args(None, &[ws_arg.clone(), ws_arg.clone(), ws_arg])
            .unwrap();
        assert_eq!(resolved_multi.len(), 3);
        assert_eq!(runtime.native_points_consumed(), expected_per_ref * 4);
    }

    #[test]
    fn repeating_a_large_workspace_arg_across_instructions_exhausts_compute_allowance() {
        let payload: Vec<u64> = (0..2_000).collect();
        let runtime = runtime_with_blobs(Blobs::default());
        runtime
            .put_on_workspace(0, IndexedValue::from_type(&payload).unwrap())
            .unwrap();

        let ws_arg = InstructionArg::Workspace(WorkspaceOffsetId::new(0));
        let mut exhausted = false;
        for _ in 0..500 {
            match runtime.resolve_args(None, std::slice::from_ref(&ws_arg)) {
                Ok(_) => {},
                Err(RuntimeError::FeeIntentComputeExceeded { .. }) => {
                    exhausted = true;
                    break;
                },
                Err(other) => panic!("expected FeeIntentComputeExceeded, got {other:?}"),
            }
        }
        assert!(
            exhausted,
            "repeatedly referencing a large workspace item must exhaust the compute allowance"
        );
        assert!(
            runtime.native_points_consumed() >
                limits::FREE_COMPUTE_GRACE_POINTS - limits::NativeExecutionPoints::PER_ENGINE_ARG_ITEM,
            "items counted before failing must be paid for"
        );
    }

    #[test]
    fn decode_return_value_bounds_item_scan_and_depth() {
        use crate::wasm::{WasmExecutionError, WasmProcess};

        let runtime = runtime_with_blobs(Blobs::default());
        let over_item_limit: Vec<u8> = vec![
            0u8;
            usize::try_from(
                limits::FREE_COMPUTE_GRACE_POINTS / limits::NativeExecutionPoints::PER_RETURN_VALUE_ITEM + 100
            )
            .unwrap()
        ];
        let mut raw = Vec::new();
        {
            let mut enc = tari_bor::minicbor::Encoder::new(&mut raw);
            enc.array(over_item_limit.len() as u64).unwrap();
            for &v in &over_item_limit {
                enc.u8(v).unwrap();
            }
        }

        let err = WasmProcess::decode_return_value_for(&runtime, &raw).unwrap_err();
        assert!(matches!(
            err,
            WasmExecutionError::RuntimeError(RuntimeError::FeeIntentComputeExceeded { .. })
        ));
        assert!(
            runtime.native_points_consumed() >
                limits::FREE_COMPUTE_GRACE_POINTS - limits::NativeExecutionPoints::PER_RETURN_VALUE_ITEM,
            "the bounded item scan must charge for the affordable items it read before failing"
        );

        let runtime_depth = runtime_with_blobs(Blobs::default());
        let depth = limits::MAX_CBOR_NESTING_DEPTH + 1;
        let mut deep_raw = vec![0x81; depth];
        deep_raw.push(0xf6);
        let depth_err = WasmProcess::decode_return_value_for(&runtime_depth, &deep_raw).unwrap_err();
        assert!(matches!(depth_err, WasmExecutionError::EncodingError(_)));
    }

    #[test]
    fn unreadable_cbor_is_charged_for_the_items_read_before_it() {
        use crate::wasm::{WasmExecutionError, WasmProcess};

        const READABLE: u64 = 200;
        // An array claiming one more element than it holds, then a reserved head the scan cannot read.
        let mut malformed = Vec::new();
        {
            let mut enc = tari_bor::minicbor::Encoder::new(&mut malformed);
            enc.array(READABLE + 1).unwrap();
            for _ in 0..READABLE {
                enc.u8(0).unwrap();
            }
        }
        malformed.push(0x1c);
        // The array, its readable elements and the unreadable one.
        let items_read = READABLE + 2;

        let runtime = runtime_with_blobs(Blobs::default());
        let err = WasmProcess::decode_return_value_for(&runtime, &malformed).unwrap_err();
        assert!(matches!(err, WasmExecutionError::EncodingError(_)), "{err:?}");
        assert_eq!(
            runtime.native_points_consumed(),
            limits::return_value_points(malformed.len() as u64, items_read)
        );

        let runtime = runtime_with_blobs(Blobs::default());
        let err = runtime.charge_engine_arg(&malformed).unwrap_err();
        assert!(matches!(err, EngineArgChargeError::Unreadable(_)), "{err:?}");
        assert_eq!(
            runtime.native_points_consumed(),
            engine_arg_points(malformed.len() as u64, items_read)
        );

        // Nesting past the depth bound is charged for every container read up to it.
        let mut too_deep = vec![0x81u8; limits::MAX_CBOR_NESTING_DEPTH + 1];
        too_deep.push(0xf6);
        let runtime = runtime_with_blobs(Blobs::default());
        runtime.charge_engine_arg(&too_deep).unwrap_err();
        assert_eq!(
            runtime.native_points_consumed(),
            engine_arg_points(too_deep.len() as u64, limits::MAX_CBOR_NESTING_DEPTH as u64 + 1)
        );
    }

    #[test]
    fn stealth_transfer_and_pay_fee_enforce_limits_before_substate_reads_and_auth() {
        let valid =
            StealthTransferStatement::revealed_only(Amount::new(1), Amount::new(1), RistrettoPublicKeyBytes::default());
        let mut too_many_inputs = valid.clone();
        too_many_inputs.inputs_statement.inputs = (0..=limits::STEALTH_LIMITS.max_inputs)
            .map(|_| StealthInput::new(PedersenCommitmentBytes::from_bytes(&[0u8; 32]).unwrap()))
            .collect();

        let runtime = runtime_with_blobs(Blobs::default());
        assert!(matches!(
            runtime.stealth_transfer(TARI_TOKEN.into(), too_many_inputs.clone(), None),
            Err(RuntimeError::ArgumentValidationError(
                ArgumentValidationError::MaxStealthInputsExceeded { .. }
            ))
        ));

        let runtime = runtime_with_blobs(Blobs::default());
        let vault_id = tari_template_lib::types::VaultId::new([1u8; 32].into());
        runtime.tracker.write_with(|state| {
            state
                .current_call_scope_mut()
                .unwrap()
                .add_substate_to_scope(vault_id.into())
                .unwrap();
        });
        let pay_fee_args = EngineArgs::from(vec![
            tari_bor::encode(&PayFeeArg {
                amount: Amount::zero(),
                statement: Some(too_many_inputs),
            })
            .unwrap()
            .into(),
        ]);
        // The statement decoder bounds inputs at `MAX_STEALTH_INPUTS`, which is `STEALTH_LIMITS.max_inputs`, so an
        // over-limit `PayFeeArg` is refused while decoding, before the vault is read.
        assert!(matches!(
            runtime.vault_invoke(VaultRef::Ref(vault_id), VaultAction::PayFee, pay_fee_args),
            Err(RuntimeError::InvalidArgument { argument, .. }) if argument.ends_with("PayFeeArg")
        ));
    }

    #[test]
    fn set_stealth_utxos_freeze_rejects_burnt_utxo() {
        use tari_engine_types::{Utxo, resource::Resource};
        use tari_template_lib::{args::SetFreezeStealthUtxosArg, types::UtxoId};

        let runtime = runtime_with_blobs(Blobs::default());
        let resource_address = ResourceAddress::new([21u8; 32].into());
        let burnt_utxo_id = UtxoId::from_array([22u8; UtxoId::LENGTH]);
        let burnt_address = UtxoAddress::new(resource_address, burnt_utxo_id);

        let resource = Resource::new(
            ResourceType::Stealth,
            SubstateOwnerRule::None,
            ResourceAccessRules::new().freezable(AccessRule::AllowAll, UpdateRule::Locked),
            Metadata::new(),
            None,
            None,
            0,
            false,
        );

        runtime
            .push_call_frame(
                PushCallFrame::Static {
                    template_address: TemplateAddress::default(),
                    module_name: "Test".to_string(),
                    arg_scope: IndexedWellKnownTypes::new(),
                    entity_id: EntityId::default(),
                },
                None,
            )
            .unwrap();
        runtime.tracker.write_with(|state| {
            state.new_substate(resource_address, resource).unwrap();
            state
                .new_substate(burnt_address.clone(), Utxo {
                    output: None,
                    is_frozen: false,
                })
                .unwrap();
        });

        for (freeze, expected_op) in [(true, "freeze_stealth_utxo"), (false, "unfreeze_stealth_utxo")] {
            let arg = EngineArgs::from(vec![
                tari_bor::encode(&SetFreezeStealthUtxosArg {
                    utxos: vec![burnt_utxo_id],
                    freeze,
                })
                .unwrap()
                .into(),
            ]);
            let err = runtime
                .resource_invoke(
                    ResourceRef::Ref(resource_address),
                    ResourceAction::SetStealthUtxosFreeze,
                    arg,
                )
                .unwrap_err();
            assert!(
                matches!(
                    err,
                    RuntimeError::InvalidOpStealthUtxoBurnt {
                        op,
                        resource_address: actual_res,
                        utxo_id: actual_id,
                    } if op == expected_op && actual_res == resource_address && actual_id == burnt_utxo_id
                ),
                "freeze={freeze} returned unexpected error: {err:?}"
            );
        }
    }

    #[test]
    fn confidential_withdraw_and_take_confidential_enforce_limits_before_substate_reads_and_auth() {
        let valid_proof = ConfidentialWithdrawProof::revealed_withdraw(Amount::new(1));
        let mut too_many_inputs = valid_proof.clone();
        too_many_inputs.inputs = (0..=limits::CONFIDENTIAL_LIMITS.max_inputs)
            .map(|_| PedersenCommitmentBytes::zero())
            .collect();

        let runtime = runtime_with_blobs(Blobs::default());
        let vault_id = tari_template_lib::types::VaultId::new([1u8; 32].into());
        let bucket_id = BucketId::from(1u32);
        runtime.tracker.write_with(|state| {
            state
                .current_call_scope_mut()
                .unwrap()
                .add_substate_to_scope(vault_id.into())
                .unwrap();
        });

        // The proof decoder bounds inputs at `MAX_CONFIDENTIAL_INPUTS`, which is `CONFIDENTIAL_LIMITS.max_inputs`, so
        // an over-limit proof is refused while decoding, before the vault or bucket is read.
        let over_limit_withdraw_args = EngineArgs::from(vec![
            tari_bor::encode(&VaultWithdrawArg::Confidential {
                proof: Box::new(too_many_inputs.clone()),
            })
            .unwrap()
            .into(),
        ]);
        assert!(matches!(
            runtime.vault_invoke(VaultRef::Ref(vault_id), VaultAction::Withdraw, over_limit_withdraw_args),
            Err(RuntimeError::InvalidArgument { argument, .. }) if argument.ends_with("VaultWithdrawArg")
        ));

        let over_limit_take_args = EngineArgs::from(vec![tari_bor::encode(&too_many_inputs).unwrap().into()]);
        assert!(matches!(
            runtime.bucket_invoke(
                BucketRef::Ref(bucket_id),
                BucketAction::TakeConfidential,
                over_limit_take_args
            ),
            Err(RuntimeError::InvalidArgument { argument, .. }) if argument.ends_with("ConfidentialWithdrawProof")
        ));

        // Exhaust the per-transaction confidential withdraw count and verify both `VaultAction::Withdraw` and
        // `BucketAction::TakeConfidential` reject before reading the non-existent vault or bucket.
        runtime.tracker.write_with(|state| {
            for _ in 0..limits::CONFIDENTIAL_LIMITS.max_withdraws_per_transaction {
                state.admit_confidential_withdraw(&valid_proof).unwrap();
            }
        });

        let valid_withdraw_args = EngineArgs::from(vec![
            tari_bor::encode(&VaultWithdrawArg::Confidential {
                proof: Box::new(valid_proof.clone()),
            })
            .unwrap()
            .into(),
        ]);
        assert!(matches!(
            runtime.vault_invoke(VaultRef::Ref(vault_id), VaultAction::Withdraw, valid_withdraw_args),
            Err(RuntimeError::ArgumentValidationError(
                ArgumentValidationError::MaxConfidentialWithdrawsPerTransactionExceeded { .. }
            ))
        ));

        let valid_take_args = EngineArgs::from(vec![tari_bor::encode(&valid_proof).unwrap().into()]);
        assert!(matches!(
            runtime.bucket_invoke(
                BucketRef::Ref(bucket_id),
                BucketAction::TakeConfidential,
                valid_take_args
            ),
            Err(RuntimeError::ArgumentValidationError(
                ArgumentValidationError::MaxConfidentialWithdrawsPerTransactionExceeded { .. }
            ))
        ));
    }

    /// A runtime holding a confidential resource whose view key is a non-canonical Ristretto point (`[0xff; 32]`),
    /// plus a vault and a bucket of that resource with 10 revealed units each. Decompressing the view key returns
    /// `InvariantError`, so a call that gets past it never decompressed the key.
    fn runtime_with_malformed_view_key_resource() -> (
        RuntimeInterfaceImpl<MemoryStateStore, DummyTemplateProvider>,
        tari_template_lib::types::VaultId,
        BucketId,
    ) {
        use tari_engine_types::{resource::Resource, vault::Vault};

        let runtime = runtime_with_blobs(Blobs::default());
        let resource_address = ResourceAddress::new([2u8; 32].into());
        let vault_id = tari_template_lib::types::VaultId::new([3u8; 32].into());

        let malformed_view_key = RistrettoPublicKeyBytes::from_bytes(&[0xff; 32]).unwrap();
        let resource = Resource::new(
            ResourceType::Confidential,
            SubstateOwnerRule::None,
            ResourceAccessRules::new(),
            Metadata::new(),
            Some(malformed_view_key),
            None,
            0,
            false,
        );
        let container = || ResourceContainer::Confidential {
            address: resource_address,
            commitments: Default::default(),
            revealed_amount: Amount::new(10),
            locked_commitments: Default::default(),
            locked_revealed_amount: Amount::zero(),
        };

        runtime
            .push_call_frame(
                PushCallFrame::Static {
                    template_address: TemplateAddress::default(),
                    module_name: "Test".to_string(),
                    arg_scope: IndexedWellKnownTypes::new(),
                    entity_id: EntityId::default(),
                },
                None,
            )
            .unwrap();
        let bucket_id = runtime.tracker.write_with(|state| {
            state.new_substate(resource_address, resource).unwrap();
            state.new_substate(vault_id, Vault::new(container())).unwrap();
            let bucket_id = state.id_provider().unwrap().new_bucket_id().unwrap();
            state.new_bucket(bucket_id, container()).unwrap();
            bucket_id
        });

        (runtime, vault_id, bucket_id)
    }

    #[test]
    fn confidential_vault_withdraw_charges_native_points_before_view_key_decompression() {
        use tari_engine_types::{confidential::withdraw_native_points, limits::FREE_COMPUTE_GRACE_POINTS};
        use tari_template_lib::types::{EncryptedData, stealth::UnspentOutput};

        let (runtime, vault_id, _) = runtime_with_malformed_view_key_resource();

        // A change output is checked against the view key, so this proof needs the key decompressed.
        let mut proof = ConfidentialWithdrawProof::revealed_withdraw(Amount::new(1));
        proof.output_proof.change_statement = Some(UnspentOutput {
            commitment: PedersenCommitmentBytes::zero(),
            sender_public_nonce: RistrettoPublicKeyBytes::zero(),
            encrypted_data: EncryptedData::empty(),
            minimum_value_promise: 0,
            viewable_balance_proof: None,
        });
        assert_eq!(proof.output_proof.num_confidential_outputs(), 1);
        let withdraw_cost = withdraw_native_points(&proof, true);
        assert!(withdraw_cost > 0);

        // Leave the tracker just 1 point short of `withdraw_cost`.
        let pre_charge = FREE_COMPUTE_GRACE_POINTS - withdraw_cost + 1;
        runtime.tracker.charge_native_execution(pre_charge).unwrap();

        let withdraw_args = EngineArgs::from(vec![
            tari_bor::encode(&VaultWithdrawArg::Confidential { proof: Box::new(proof) })
                .unwrap()
                .into(),
        ]);
        let err = runtime
            .vault_invoke(VaultRef::Ref(vault_id), VaultAction::Withdraw, withdraw_args)
            .unwrap_err();
        assert!(
            matches!(err, RuntimeError::FeeIntentComputeExceeded { .. }),
            "expected FeeIntentComputeExceeded before view-key decompression, got {err:?}"
        );
    }

    #[test]
    fn revealed_only_confidential_withdraw_and_take_skip_view_key_decompression() {
        use tari_engine_types::confidential::withdraw_native_points;

        let (runtime, vault_id, bucket_id) = runtime_with_malformed_view_key_resource();

        // A revealed-only proof prices at zero native points and has no outputs to check against the view key, so
        // both paths complete without decompressing the malformed key.
        let proof = ConfidentialWithdrawProof::revealed_withdraw(Amount::new(1));
        assert_eq!(withdraw_native_points(&proof, true), 0);
        assert_eq!(proof.output_proof.num_confidential_outputs(), 0);

        let withdraw_args = EngineArgs::from(vec![
            tari_bor::encode(&VaultWithdrawArg::Confidential {
                proof: Box::new(proof.clone()),
            })
            .unwrap()
            .into(),
        ]);
        runtime
            .vault_invoke(VaultRef::Ref(vault_id), VaultAction::Withdraw, withdraw_args)
            .unwrap();

        let take_args = EngineArgs::from(vec![tari_bor::encode(&proof).unwrap().into()]);
        runtime
            .bucket_invoke(BucketRef::Ref(bucket_id), BucketAction::TakeConfidential, take_args)
            .unwrap();
    }

    #[test]
    fn restricted_frames_forbid_auth_hooks_spend_scripts_and_stealth_transfers() {
        let statement =
            StealthTransferStatement::revealed_only(Amount::new(1), Amount::new(1), RistrettoPublicKeyBytes::default());
        let vault_id = tari_template_lib::types::VaultId::new([1u8; 32].into());
        let pay_fee_args = EngineArgs::from(vec![
            tari_bor::encode(&PayFeeArg {
                amount: Amount::zero(),
                statement: Some(statement.clone()),
            })
            .unwrap()
            .into(),
        ]);

        let runtime = runtime_with_blobs(Blobs::default());
        runtime
            .push_call_frame(
                PushCallFrame::Static {
                    template_address: TemplateAddress::default(),
                    module_name: "Test".to_string(),
                    arg_scope: IndexedWellKnownTypes::new(),
                    entity_id: EntityId::default(),
                },
                Some(FrameWriteMode::ReadOnly),
            )
            .unwrap();
        runtime.tracker.write_with(|state| {
            state
                .current_call_scope_mut()
                .unwrap()
                .add_substate_to_referenced(vault_id.into());
        });

        assert!(matches!(
            runtime.enforce_frame_restrictions("invoke_resource_access_hook"),
            Err(RuntimeError::ForbiddenInReadOnlyContext {
                operation: "invoke_resource_access_hook"
            })
        ));
        assert!(matches!(
            runtime.enforce_frame_restrictions("evaluate_spend_script"),
            Err(RuntimeError::ForbiddenInReadOnlyContext {
                operation: "evaluate_spend_script"
            })
        ));
        assert!(matches!(
            runtime.stealth_transfer(TARI_TOKEN.into(), statement.clone(), None),
            Err(RuntimeError::WriteInReadOnlyContext)
        ));
        assert!(matches!(
            runtime.vault_invoke(VaultRef::Ref(vault_id), VaultAction::PayFee, pay_fee_args.clone()),
            Err(RuntimeError::WriteInReadOnlyContext)
        ));
        runtime.pop_call_frame(&IndexedWellKnownTypes::new()).unwrap();

        runtime
            .push_call_frame(
                PushCallFrame::Static {
                    template_address: TemplateAddress::default(),
                    module_name: "Test".to_string(),
                    arg_scope: IndexedWellKnownTypes::new(),
                    entity_id: EntityId::default(),
                },
                Some(FrameWriteMode::OwnComponent),
            )
            .unwrap();
        runtime.tracker.write_with(|state| {
            state
                .current_call_scope_mut()
                .unwrap()
                .add_substate_to_referenced(vault_id.into());
        });

        assert!(matches!(
            runtime.enforce_frame_restrictions("invoke_resource_access_hook"),
            Err(RuntimeError::ForbiddenInRestrictedFrame {
                operation: "invoke_resource_access_hook",
                ..
            })
        ));
        assert!(matches!(
            runtime.enforce_frame_restrictions("evaluate_spend_script"),
            Err(RuntimeError::ForbiddenInRestrictedFrame {
                operation: "evaluate_spend_script",
                ..
            })
        ));
        assert!(matches!(
            runtime.stealth_transfer(TARI_TOKEN.into(), statement, None),
            Err(RuntimeError::WriteOutsideOwnComponent { .. })
        ));
        assert!(matches!(
            runtime.vault_invoke(VaultRef::Ref(vault_id), VaultAction::PayFee, pay_fee_args),
            Err(RuntimeError::WriteOutsideOwnComponent { .. })
        ));
    }

    fn pay_fee_from_new_bucket(
        runtime: &RuntimeInterfaceImpl<MemoryStateStore, DummyTemplateProvider>,
        workspace_id: WorkspaceId,
        container: ResourceContainer,
    ) -> (BucketId, Result<(), RuntimeError>) {
        let bucket_id = runtime.tracker.write_with(|state| {
            let bucket_id = state.new_bucket_id().unwrap();
            state.new_bucket(bucket_id, container).unwrap();
            bucket_id
        });
        runtime
            .put_on_workspace(workspace_id, IndexedValue::from_type(&bucket_id).unwrap())
            .unwrap();
        let result = runtime.pay_fee(PayFee::FromBucket {
            bucket: WorkspaceOffsetId::new(workspace_id),
        });
        (bucket_id, result)
    }

    #[test]
    fn pay_fee_from_an_empty_tari_bucket_consumes_it_and_records_no_payment() {
        let runtime = runtime_with_blobs(Blobs::default());

        let (bucket_id, result) =
            pay_fee_from_new_bucket(&runtime, 0, ResourceContainer::stealth(TARI_TOKEN, Amount::zero()));
        result.unwrap();
        assert_eq!(runtime.tracker.total_fee_payments(), 0);
        assert!(runtime.tracker.read_with(|state| state.get_bucket(bucket_id).is_err()));

        let (_, result) = pay_fee_from_new_bucket(&runtime, 1, ResourceContainer::stealth(TARI_TOKEN, Amount::new(7)));
        result.unwrap();
        assert_eq!(runtime.tracker.total_fee_payments(), 7);
    }

    #[test]
    fn pay_fee_from_an_empty_non_tari_bucket_is_rejected() {
        let runtime = runtime_with_blobs(Blobs::default());
        let other_resource = ResourceAddress::new([9u8; 32].into());

        let (_, result) = pay_fee_from_new_bucket(
            &runtime,
            0,
            ResourceContainer::public_fungible(other_resource, Amount::zero()),
        );
        assert!(matches!(result, Err(RuntimeError::InvalidArgument { .. })));
        assert_eq!(runtime.tracker.total_fee_payments(), 0);
    }

    fn create_test_component(
        runtime: &RuntimeInterfaceImpl<MemoryStateStore, DummyTemplateProvider>,
        template_address: TemplateAddress,
        module_name: &str,
        entity_id: EntityId,
    ) -> ComponentAddress {
        runtime
            .push_call_frame(
                PushCallFrame::Static {
                    template_address,
                    module_name: module_name.to_string(),
                    arg_scope: IndexedWellKnownTypes::new(),
                    entity_id,
                },
                None,
            )
            .unwrap();
        let component_address = runtime
            .tracker
            .new_component(
                tari_bor::Value::Null,
                SubstateOwnerRule::None,
                ComponentAccessRules::new(),
                None,
                UpdateRule::Locked,
            )
            .unwrap();
        runtime
            .pop_call_frame(
                &IndexedWellKnownTypes::from_value(&tari_bor::to_value(&component_address).unwrap()).unwrap(),
            )
            .unwrap();
        component_address
    }

    #[test]
    fn get_template_address_succeeds_on_write_locked_current_component_and_migration() {
        let runtime = runtime_with_blobs(Blobs::default());
        let expected_template = TemplateAddress::from_array([7u8; 32]);
        let other_template = TemplateAddress::from_array([8u8; 32]);
        let migrated_template = TemplateAddress::from_array([9u8; 32]);
        let entity_id = EntityId::default();

        let component_address = create_test_component(&runtime, expected_template, "Test", entity_id);
        let other_component_address = create_test_component(
            &runtime,
            other_template,
            "Other",
            EntityId::new([1u8; EntityId::LENGTH]),
        );

        // Simulate a `&mut self` component method holding a Write lock on `component_address`.
        let component_lock = runtime.lock_component(component_address, LockFlag::Write).unwrap();
        let arg_scope =
            IndexedWellKnownTypes::from_value(&tari_bor::to_value(&other_component_address).unwrap()).unwrap();
        runtime
            .push_call_frame(
                PushCallFrame::ForComponent {
                    template_address: expected_template,
                    module_name: "Test".to_string(),
                    component_scope: IndexedWellKnownTypes::new(),
                    component_lock,
                    arg_scope: Box::new(arg_scope),
                    entity_id,
                },
                None,
            )
            .unwrap();

        let invoke_res = runtime
            .component_invoke(
                ComponentRef::Ref(component_address),
                ComponentAction::GetTemplateAddress,
                EngineArgs::default(),
            )
            .unwrap();
        let actual_template: TemplateAddress = invoke_res.decode().unwrap();
        assert_eq!(actual_template, expected_template);

        // Querying a different component's template address still acquires and releases a temporary read lock.
        let other_res = runtime
            .component_invoke(
                ComponentRef::Ref(other_component_address),
                ComponentAction::GetTemplateAddress,
                EngineArgs::default(),
            )
            .unwrap();
        let actual_other_template: TemplateAddress = other_res.decode().unwrap();
        assert_eq!(actual_other_template, other_template);

        // Ensure the frame's write lock is still held so subsequent `SetState` and `pop_call_frame` succeed.
        runtime
            .component_invoke(
                ComponentRef::Ref(component_address),
                ComponentAction::SetState,
                EngineArgs::from(vec![tari_bor::encode(&tari_bor::Value::Null).unwrap().into()]),
            )
            .unwrap();
        runtime.pop_call_frame(&IndexedWellKnownTypes::new()).unwrap();

        // Simulate a component migration holding a Write lock on `component_address`.
        let migration_lock = runtime.lock_component(component_address, LockFlag::Write).unwrap();
        runtime
            .push_call_frame(
                PushCallFrame::MigrationContext {
                    template_address: migrated_template,
                    module_name: "TestV2".to_string(),
                    component_scope: IndexedWellKnownTypes::new(),
                    component_lock: migration_lock,
                    arg_scope: Box::new(IndexedWellKnownTypes::new()),
                    entity_id,
                },
                None,
            )
            .unwrap();
        runtime.update_component_template(migrated_template).unwrap();

        let migrated_res = runtime
            .component_invoke(
                ComponentRef::Ref(component_address),
                ComponentAction::GetTemplateAddress,
                EngineArgs::default(),
            )
            .unwrap();
        let actual_migrated: TemplateAddress = migrated_res.decode().unwrap();
        assert_eq!(actual_migrated, migrated_template);
        runtime.pop_call_frame(&IndexedWellKnownTypes::new()).unwrap();
    }

    #[test]
    fn component_get_state_enforces_call_scope_and_charges_component_load() {
        use tari_engine_types::limits::component_load_points;

        let runtime = runtime_with_blobs(Blobs::default());
        let template_address = TemplateAddress::from_array([11u8; 32]);
        let entity_id = EntityId::default();

        let caller_component = create_test_component(&runtime, template_address, "Caller", entity_id);

        // Create a target component with a non-trivial state payload.
        let payload: Vec<u64> = (0..64).collect();
        let target_state = tari_bor::to_value(&payload).unwrap();
        let counted = target_state
            .count_items_bounded(u64::MAX, IndexedWellKnownTypes::indexes_tag)
            .unwrap();
        let expected_load_points = component_load_points(counted.items, counted.counted_tags);
        assert!(expected_load_points > 0);

        let target_entity = EntityId::new([2u8; EntityId::LENGTH]);
        runtime
            .push_call_frame(
                PushCallFrame::Static {
                    template_address,
                    module_name: "Target".to_string(),
                    arg_scope: IndexedWellKnownTypes::new(),
                    entity_id: target_entity,
                },
                None,
            )
            .unwrap();
        let target_component = runtime
            .tracker
            .new_component(
                target_state.clone(),
                SubstateOwnerRule::None,
                ComponentAccessRules::new(),
                None,
                UpdateRule::Locked,
            )
            .unwrap();
        runtime
            .pop_call_frame(
                &IndexedWellKnownTypes::from_value(&tari_bor::to_value(&target_component).unwrap()).unwrap(),
            )
            .unwrap();

        // Push a frame for `caller_component` WITHOUT `target_component` in scope.
        let caller_lock = runtime.lock_component(caller_component, LockFlag::Write).unwrap();
        runtime
            .push_call_frame(
                PushCallFrame::ForComponent {
                    template_address,
                    module_name: "Caller".to_string(),
                    component_scope: IndexedWellKnownTypes::new(),
                    component_lock: caller_lock,
                    arg_scope: Box::new(IndexedWellKnownTypes::new()),
                    entity_id,
                },
                None,
            )
            .unwrap();

        let points_before = runtime.native_points_consumed();
        let err = runtime
            .component_invoke(
                ComponentRef::Ref(target_component),
                ComponentAction::GetState,
                EngineArgs::default(),
            )
            .unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::SubstateOutOfScope { id } if id == SubstateId::Component(target_component)
        ));
        assert_eq!(runtime.native_points_consumed(), points_before);
        runtime.pop_call_frame(&IndexedWellKnownTypes::new()).unwrap();

        // Push a frame for `caller_component` WITH `target_component` in `arg_scope`.
        let caller_lock = runtime.lock_component(caller_component, LockFlag::Write).unwrap();
        let arg_scope = IndexedWellKnownTypes::from_value(&tari_bor::to_value(&target_component).unwrap()).unwrap();
        runtime
            .push_call_frame(
                PushCallFrame::ForComponent {
                    template_address,
                    module_name: "Caller".to_string(),
                    component_scope: IndexedWellKnownTypes::new(),
                    component_lock: caller_lock,
                    arg_scope: Box::new(arg_scope),
                    entity_id,
                },
                None,
            )
            .unwrap();

        let points_before = runtime.native_points_consumed();
        let res = runtime
            .component_invoke(
                ComponentRef::Ref(target_component),
                ComponentAction::GetState,
                EngineArgs::default(),
            )
            .unwrap();
        let decoded_state: tari_bor::Value = res.decode().unwrap();
        assert_eq!(decoded_state, target_state);
        assert_eq!(runtime.native_points_consumed() - points_before, expected_load_points);
        runtime.pop_call_frame(&IndexedWellKnownTypes::new()).unwrap();
    }

    #[test]
    fn component_get_template_address_enforces_call_scope() {
        let runtime = runtime_with_blobs(Blobs::default());
        let caller_template = TemplateAddress::from_array([12u8; 32]);
        let target_template = TemplateAddress::from_array([13u8; 32]);
        let entity_id = EntityId::default();

        let caller_component = create_test_component(&runtime, caller_template, "Caller", entity_id);
        let target_component = create_test_component(
            &runtime,
            target_template,
            "Target",
            EntityId::new([3u8; EntityId::LENGTH]),
        );

        // Push a frame for `caller_component` WITHOUT `target_component` in scope.
        let caller_lock = runtime.lock_component(caller_component, LockFlag::Write).unwrap();
        runtime
            .push_call_frame(
                PushCallFrame::ForComponent {
                    template_address: caller_template,
                    module_name: "Caller".to_string(),
                    component_scope: IndexedWellKnownTypes::new(),
                    component_lock: caller_lock,
                    arg_scope: Box::new(IndexedWellKnownTypes::new()),
                    entity_id,
                },
                None,
            )
            .unwrap();

        // Reading the currently executing component's own template address still succeeds.
        let own_res = runtime
            .component_invoke(
                ComponentRef::Ref(caller_component),
                ComponentAction::GetTemplateAddress,
                EngineArgs::default(),
            )
            .unwrap();
        let actual_own: TemplateAddress = own_res.decode().unwrap();
        assert_eq!(actual_own, caller_template);

        // Reading an out-of-scope external component fails with `SubstateOutOfScope` and consumes no native points.
        let points_before = runtime.native_points_consumed();
        let err = runtime
            .component_invoke(
                ComponentRef::Ref(target_component),
                ComponentAction::GetTemplateAddress,
                EngineArgs::default(),
            )
            .unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::SubstateOutOfScope { id } if id == SubstateId::Component(target_component)
        ));
        assert_eq!(runtime.native_points_consumed(), points_before);
        runtime.pop_call_frame(&IndexedWellKnownTypes::new()).unwrap();

        // Static frame without `target_component` in scope also rejects with `SubstateOutOfScope`.
        runtime
            .push_call_frame(
                PushCallFrame::Static {
                    template_address: caller_template,
                    module_name: "Caller".to_string(),
                    arg_scope: IndexedWellKnownTypes::new(),
                    entity_id,
                },
                None,
            )
            .unwrap();
        let points_before = runtime.native_points_consumed();
        let err = runtime
            .component_invoke(
                ComponentRef::Ref(target_component),
                ComponentAction::GetTemplateAddress,
                EngineArgs::default(),
            )
            .unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::SubstateOutOfScope { id } if id == SubstateId::Component(target_component)
        ));
        assert_eq!(runtime.native_points_consumed(), points_before);
        runtime.pop_call_frame(&IndexedWellKnownTypes::new()).unwrap();

        // Push a frame for `caller_component` WITH `target_component` in `arg_scope`.
        let caller_lock = runtime.lock_component(caller_component, LockFlag::Write).unwrap();
        let arg_scope = IndexedWellKnownTypes::from_value(&tari_bor::to_value(&target_component).unwrap()).unwrap();
        runtime
            .push_call_frame(
                PushCallFrame::ForComponent {
                    template_address: caller_template,
                    module_name: "Caller".to_string(),
                    component_scope: IndexedWellKnownTypes::new(),
                    component_lock: caller_lock,
                    arg_scope: Box::new(arg_scope),
                    entity_id,
                },
                None,
            )
            .unwrap();

        let res = runtime
            .component_invoke(
                ComponentRef::Ref(target_component),
                ComponentAction::GetTemplateAddress,
                EngineArgs::default(),
            )
            .unwrap();
        let actual_target: TemplateAddress = res.decode().unwrap();
        assert_eq!(actual_target, target_template);
        runtime.pop_call_frame(&IndexedWellKnownTypes::new()).unwrap();
    }

    fn create_account_component_with_method_rules(
        runtime: &RuntimeInterfaceImpl<MemoryStateStore, DummyTemplateProvider>,
        entity_id: EntityId,
        access_rules: ComponentAccessRules,
    ) -> ComponentAddress {
        runtime
            .push_call_frame(
                PushCallFrame::Static {
                    template_address: ACCOUNT_TEMPLATE_ADDRESS,
                    module_name: "Account".to_string(),
                    arg_scope: IndexedWellKnownTypes::new(),
                    entity_id,
                },
                None,
            )
            .unwrap();
        let component_address = runtime
            .tracker
            .new_component(
                tari_bor::Value::Null,
                SubstateOwnerRule::None,
                access_rules,
                None,
                UpdateRule::AccessRule(AccessRule::AllowAll),
            )
            .unwrap();
        runtime
            .pop_call_frame(
                &IndexedWellKnownTypes::from_value(&tari_bor::to_value(&component_address).unwrap()).unwrap(),
            )
            .unwrap();
        component_address
    }

    fn push_nft_faucet_migration_frame(
        runtime: &RuntimeInterfaceImpl<MemoryStateStore, DummyTemplateProvider>,
        entity_id: EntityId,
        component_address: ComponentAddress,
    ) -> LockedSubstate {
        let migration_lock = runtime.lock_component(component_address, LockFlag::Write).unwrap();
        runtime
            .push_call_frame(
                PushCallFrame::MigrationContext {
                    template_address: NFT_FAUCET_TEMPLATE_ADDRESS,
                    module_name: "NftFaucet".to_string(),
                    component_scope: IndexedWellKnownTypes::new(),
                    component_lock: migration_lock.clone(),
                    arg_scope: Box::new(IndexedWellKnownTypes::new()),
                    entity_id,
                },
                None,
            )
            .unwrap();
        migration_lock
    }

    #[test]
    fn update_component_template_rejects_method_rules_absent_from_new_template() {
        let runtime = runtime_with_blobs(Blobs::default());
        let entity_id = EntityId::default();

        // `"balance"` exists on `Account` and is absent from `NftFaucet`; `"mint"` exists on `NftFaucet`.
        let access_rules = ComponentAccessRules::new()
            .add_method_rule("balance", AccessRule::DenyAll)
            .add_method_rule("mint", AccessRule::AllowAll);
        let component_address = create_account_component_with_method_rules(&runtime, entity_id, access_rules.clone());
        let migration_lock = push_nft_faucet_migration_frame(&runtime, entity_id, component_address);

        let err = runtime
            .update_component_template(NFT_FAUCET_TEMPLATE_ADDRESS)
            .unwrap_err();
        assert!(
            matches!(
                &err,
                RuntimeError::InvalidMethodAccessRule { details, .. } if details.contains("'balance'")
            ),
            "unexpected error: {err:?}"
        );
        runtime
            .tracker
            .read_with(|state| {
                let component = state.get_component(&migration_lock).unwrap();
                assert_eq!(*component.template_address(), ACCOUNT_TEMPLATE_ADDRESS);
                assert_eq!(*component.access_rules(), access_rules);
                Ok::<_, RuntimeError>(())
            })
            .unwrap();
        runtime.pop_call_frame(&IndexedWellKnownTypes::new()).unwrap();
    }

    #[test]
    fn update_component_template_keeps_method_rules_present_on_new_template() {
        let runtime = runtime_with_blobs(Blobs::default());
        let entity_id = EntityId::default();

        let access_rules = ComponentAccessRules::new().add_method_rule("mint", AccessRule::DenyAll);
        let component_address = create_account_component_with_method_rules(&runtime, entity_id, access_rules.clone());
        let migration_lock = push_nft_faucet_migration_frame(&runtime, entity_id, component_address);

        runtime.update_component_template(NFT_FAUCET_TEMPLATE_ADDRESS).unwrap();
        runtime
            .tracker
            .read_with(|state| {
                let component = state.get_component(&migration_lock).unwrap();
                assert_eq!(*component.template_address(), NFT_FAUCET_TEMPLATE_ADDRESS);
                assert_eq!(*component.access_rules(), access_rules);
                Ok::<_, RuntimeError>(())
            })
            .unwrap();
        runtime.pop_call_frame(&IndexedWellKnownTypes::new()).unwrap();
    }
}
