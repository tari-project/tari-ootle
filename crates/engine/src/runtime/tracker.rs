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

use indexmap::IndexMap;
use log::*;
use tari_engine_types::{
    commit_result::{FinalizeResult, RejectReason, TransactionResult},
    component::{Component, ComponentBody, ComponentHeader},
    events::Event,
    fees::FeeSource,
    indexed_value::{IndexedValue, IndexedWellKnownTypes},
    limits,
    lock::LockFlag,
    logs::LogEntry,
    substate::{Substate, SubstateId, SubstateValue},
    transaction_receipt::FinalizeOutcome,
    virtual_substate::VirtualSubstates,
};
use tari_ootle_common_types::Epoch;
use tari_ootle_transaction::TransactionWeight;
use tari_template_lib::{
    models::ComponentAddressAllocation,
    types::{
        ComponentAddress,
        Hash32,
        Metadata,
        SubstateOwnerRule,
        TemplateAddress,
        access_rules::ComponentAccessRules,
    },
};

use crate::{
    runtime::{
        RuntimeError,
        locking::LockedSubstate,
        scope::{CallScope, PushCallFrame},
        working_state::WorkingState,
        workspace::Workspace,
    },
    state_store::StateReader,
};

const LOG_TARGET: &str = "tari::ootle::engine::runtime::state_tracker";

#[derive(Debug)]
pub struct StateTracker<TStore> {
    working_state: Option<WorkingState<TStore>>,
    fee_checkpoint: Option<WorkingState<TStore>>,
    transaction_weight: TransactionWeight,
}

impl<TStore: StateReader> StateTracker<TStore> {
    pub fn new(
        state_store: TStore,
        virtual_substates: VirtualSubstates,
        initial_call_scope: CallScope,
        transaction_hash: Hash32,
        transaction_weight: TransactionWeight,
    ) -> Self {
        Self {
            working_state: Some(WorkingState::new(
                state_store,
                virtual_substates,
                initial_call_scope,
                transaction_hash,
            )),
            fee_checkpoint: None,
            transaction_weight,
        }
    }

    pub fn get_transaction_weight(&self) -> TransactionWeight {
        self.transaction_weight
    }

    pub fn get_current_epoch(&self) -> Result<Epoch, RuntimeError> {
        self.read_with(|state| state.get_current_epoch())
    }

    pub fn get_current_epoch_hash(&self) -> Result<Hash32, RuntimeError> {
        self.read_with(|state| state.get_current_epoch_hash())
    }

    pub fn get_pseudorandom_bytes(&self, length: usize) -> Result<Vec<u8>, RuntimeError> {
        self.read_with(|state| {
            let id_provider = state.id_provider()?;
            // TODO: epoch_hash is a bad source of entropy. Agreeing on randomness at a consensus level is challenging
            // in multi-sharded consensus.
            let epoch_hash = state.get_current_epoch_hash()?;
            let bytes = id_provider.get_random_bytes(&epoch_hash, length)?;
            Ok(bytes)
        })
    }

    pub fn add_event(&mut self, event: Event) -> Result<(), RuntimeError> {
        debug!(target: LOG_TARGET, "Emit: {event}");
        self.write_with(|state| state.push_event(event))
    }

    pub fn add_log(&mut self, log: LogEntry) -> Result<(), RuntimeError> {
        self.write_with(|state| state.push_log(log))
    }

    /// Returns `true` the first time a given template address is seen within this transaction's
    /// state, `false` thereafter. Callers use this to dedupe `FeeSource::TemplateLoad` charges:
    /// the validator pays the cold compile/deserialise cost at most once per template per
    /// process, so charging it on every entry over-bills the user.
    pub fn record_template_load_charge(&mut self, address: TemplateAddress) -> bool {
        self.write_with(|state| state.record_template_load_charge(address))
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        self.write_with(|state| state.take_events())
    }

    pub fn get_template_address(&self) -> Result<TemplateAddress, RuntimeError> {
        self.read_with(|state| state.current_template().map(|(a, _)| *a))
    }

    pub fn get_template_module_name(&self) -> Result<String, RuntimeError> {
        self.read_with(|state| state.current_template().map(|(_, name)| name.to_string()))
    }

    pub fn new_component(
        &mut self,
        component_state: tari_bor::Value,
        owner_rule: SubstateOwnerRule,
        access_rules: ComponentAccessRules,
        address_allocation: Option<ComponentAddressAllocation>,
    ) -> Result<ComponentAddress, RuntimeError> {
        self.write_with(|state| {
            let template_address = state.current_template().map(|(addr, _)| *addr)?;

            let component_address = match address_allocation {
                Some(address_allocation) => {
                    let alloc = state.use_allocated_address(address_allocation.id())?;
                    alloc.substate_id().as_component_address().ok_or_else(|| {
                        RuntimeError::AddressAllocationTypeMismatch {
                            id: alloc.substate_id().clone(),
                            expected: "ComponentAddress",
                        }
                    })?
                },
                None => state.id_provider()?.new_component_address()?,
            };

            let body = ComponentBody { state: component_state };
            let header = ComponentHeader {
                template_address,
                access_rules,
                owner_rule,
                entity_id: component_address.entity_id(),
            };

            let component = Component { header, body };
            let substate_id = SubstateId::Component(component_address);
            // The template address/component_id combination will not necessarily be unique so we need to check this.
            if state.substate_exists(&substate_id)? {
                return Err(RuntimeError::ComponentAlreadyExists {
                    address: component_address,
                });
            }

            let indexed = IndexedWellKnownTypes::from_value(&component.body.state)?;
            state.validate_component_state(None, &indexed)?;

            state.new_substate(substate_id.clone(), SubstateValue::Component(component))?;

            state.push_event(Event::std(
                Some(substate_id),
                template_address,
                "component",
                "created",
                Metadata::new(),
            ))?;

            debug!(target: LOG_TARGET, "New component created: {}", component_address);
            Ok(component_address)
        })
    }

    pub fn lock_substate(&mut self, address: SubstateId, lock_flag: LockFlag) -> Result<LockedSubstate, RuntimeError> {
        self.write_with(|state| match lock_flag {
            LockFlag::Read => state.read_lock_substate(address),
            LockFlag::Write => state.write_lock_substate(address),
        })
    }

    pub fn unlock_substate(&mut self, locked: LockedSubstate) -> Result<(), RuntimeError> {
        self.write_with(|state| state.unlock_substate(locked))
    }

    pub fn push_call_frame(&mut self, push_frame: PushCallFrame) -> Result<(), RuntimeError> {
        self.write_with(|state| {
            // If substates used in args are in scope for the current frame, we can bring then into scope for the new
            // frame
            trace!(
                 target: LOG_TARGET,
                "CALL FRAME before:\n{}",
                state.current_call_scope()?,
            );
            state.check_all_substates_in_scope(push_frame.arg_scope())?;

            let new_frame = push_frame.into_new_call_frame();
            trace!(target: LOG_TARGET, "NEW CALL FRAME:\n{}", new_frame.scope());

            state.push_frame(new_frame, limits::ENGINE_LIMITS.max_call_depth)
        })
    }

    pub fn pop_call_frame(&mut self) -> Result<(), RuntimeError> {
        self.write_with(|state| state.pop_frame())
    }

    pub fn take_last_instruction_output(&mut self) -> Option<IndexedValue> {
        self.write_with(|state| state.take_last_instruction_output())
    }

    pub fn with_workspace<F: FnOnce(&Workspace) -> R, R>(&self, f: F) -> R {
        self.read_with(|state| f(state.workspace()))
    }

    pub fn with_workspace_mut<F: FnOnce(&mut Workspace) -> R, R>(&mut self, f: F) -> R {
        self.write_with(|state| f(state.workspace_mut()))
    }

    pub fn add_fee_charge(&mut self, source: FeeSource, amount: u64) {
        if amount == 0 {
            debug!(target: LOG_TARGET, "Add fee: source: {:?}, amount: {}", source, amount);
            return;
        }

        self.write_with(|state| {
            debug!(target: LOG_TARGET, "Add fee: source: {:?}, amount: {}", source, amount);
            state.fee_state_mut().add_charge(source, amount);
        })
    }

    pub fn set_fee_state_dry_run(&mut self, dry_run: bool) {
        self.write_with(|state| {
            state.fee_state_mut().set_dry_run(dry_run);
        })
    }

    pub fn accumulate_wasm_points(&mut self, points: u64) {
        self.write_with(|state| state.fee_state_mut().accumulate_wasm_points(points))
    }

    pub fn accumulated_wasm_points(&self) -> u64 {
        self.read_with(|state| state.fee_state().accumulated_wasm_points())
    }

    pub fn finalize(&mut self, failure: Option<RejectReason>) -> Result<FinalizeResult, RuntimeError> {
        let failure = failure.or_else(|| {
            self.read_with(|state| {
                let fee_state = state.fee_state();
                if fee_state.is_dry_run() || fee_state.is_paid_in_full() {
                    None
                } else {
                    Some(RejectReason::InsufficientFeesPaid(format!(
                        "Required fees {} but {} paid",
                        fee_state.total_charges(),
                        fee_state.total_payments()
                    )))
                }
            })
        });

        if let Some(reason) = failure {
            let mut checkpoint_state = self.take_fee_checkpoint().ok_or(RuntimeError::NoFeeCheckpoint)?;
            // Preserve fee state across resets so that we can charge for fees incurred during execution before the
            // failure
            self.read_with(|state| {
                // Fee state in `state` includes the payments and charges from the fee transaction
                *checkpoint_state.fee_state_mut() = state.fee_state().clone();
            });
            let mut substates_to_persist = checkpoint_state.take_mutated_substates();
            // Process fees and refunds based on the fee checkpoint state
            let fee_receipt = checkpoint_state.finalize_fees_and_refunds(&mut substates_to_persist)?;

            let downed_utxos = checkpoint_state.take_downed_utxos();
            let fee_withdrawals = checkpoint_state.take_validator_fee_withdrawals();

            let mut diff =
                checkpoint_state.generate_substate_diff(substates_to_persist, downed_utxos, fee_withdrawals)?;
            let transaction_receipt = checkpoint_state.finalize_transaction_receipt(
                FinalizeOutcome::FeeIntentCommit,
                &diff,
                fee_receipt.clone(),
            )?;
            diff.up(
                SubstateId::TransactionReceipt(checkpoint_state.transaction_hash().into()),
                Substate::new(0, transaction_receipt),
            );

            return Ok(FinalizeResult::new(
                checkpoint_state.transaction_hash(),
                checkpoint_state.take_logs(),
                checkpoint_state.take_events(),
                TransactionResult::AcceptFeeRejectRest(diff, reason),
                fee_receipt,
            ));
        }

        // Finalise will always reset the state
        let mut state = self.take_working_state()?;
        // Resolve the transfers to the fee pool resource and vault refunds
        let mut substates_to_persist = state.take_mutated_substates();
        let fee_receipt = state.finalize_fees_and_refunds(&mut substates_to_persist)?;
        let downed_utxos = state.take_downed_utxos();
        let fee_withdrawals = state.take_validator_fee_withdrawals();
        let mut diff = state.generate_substate_diff(substates_to_persist, downed_utxos, fee_withdrawals)?;
        let transaction_receipt =
            state.finalize_transaction_receipt(FinalizeOutcome::Commit, &diff, fee_receipt.clone())?;
        diff.up(
            SubstateId::TransactionReceipt(state.transaction_hash().into()),
            Substate::new(0, transaction_receipt),
        );

        Ok(FinalizeResult::new(
            state.transaction_hash(),
            state.take_logs(),
            state.take_events(),
            TransactionResult::Accept(diff),
            fee_receipt,
        ))
    }

    fn take_fee_checkpoint(&mut self) -> Option<WorkingState<TStore>> {
        self.fee_checkpoint.take()
    }

    fn take_working_state(&mut self) -> Result<WorkingState<TStore>, RuntimeError> {
        self.working_state.take().ok_or_else(|| RuntimeError::InvariantError {
            function: "StateTracker::take_working_state",
            details: "Working state has already been taken (double finalize?)".to_string(),
        })
    }

    pub fn with_substates_to_persist<F: FnMut(&IndexMap<SubstateId, SubstateValue>) -> R, R>(&mut self, mut f: F) -> R {
        self.write_with(|state| f(state.mutated_substates()))
    }

    /// Counts substates in the to-persist set that did not previously exist in the state store.
    /// Used by the fee module to charge a slot-allocation premium on top of per-byte storage.
    pub fn count_newly_created_substates(&self) -> Result<usize, RuntimeError> {
        self.read_with(|state| {
            let store = state.store();
            let mut count = 0;
            for id in store.mutated_substates().keys() {
                match store.get_unmodified_substate(id) {
                    Ok(_) => {},
                    Err(RuntimeError::SubstateNotFound { .. }) => count += 1,
                    Err(e) => return Err(e),
                }
            }
            Ok(count)
        })
    }

    pub fn are_fees_paid_in_full(&self) -> bool {
        self.read_with(|state| {
            let total_payments = state.fee_state().total_payments();
            let total_charges = state.fee_state().total_charges();
            total_payments >= total_charges
        })
    }

    pub fn total_fee_payments(&self) -> u64 {
        self.read_with(|state| state.fee_state().total_payments())
    }

    pub fn total_fee_charges(&self) -> u64 {
        self.read_with(|state| state.fee_state().total_charges())
    }

    pub fn is_fee_state_dry_run(&self) -> bool {
        self.read_with(|state| state.fee_state().is_dry_run())
    }

    /// Whether the current call frame is a read-only spend-script sandbox. Used by the engine's core read-only
    /// enforcement to deny the few effectful host ops that bypass the lock layer.
    pub fn is_in_read_only_context(&self) -> bool {
        self.working_state
            .as_ref()
            .map(|state| state.is_read_only_context())
            .unwrap_or(false)
    }

    pub(super) fn read_with<R, F: FnOnce(&WorkingState<TStore>) -> R>(&self, f: F) -> R {
        f(self
            .working_state
            .as_ref()
            .expect("BUG: read_with called after finalize consumed working state"))
    }

    pub(super) fn write_with<R, F: FnOnce(&mut WorkingState<TStore>) -> R>(&mut self, f: F) -> R {
        f(self
            .working_state
            .as_mut()
            .expect("BUG: write_with called after finalize consumed working state"))
    }

    pub(super) fn is_fee_intent_checkpointed(&self) -> bool {
        self.fee_checkpoint.is_some()
    }
}

impl<TStore: StateReader + Clone> StateTracker<TStore> {
    pub fn fee_checkpoint(&mut self) -> Result<(), RuntimeError> {
        let state = self.write_with(|state| {
            // Check that the checkpoint is in a valid state
            state.validate_finalized()?;
            let checkpoint_state = state.clone();

            // After checkpointing, the main intent has a cleared workspace
            state.workspace_mut().clear_items();
            let proofs = state.workspace_mut().drain_all_proofs();
            for proof_id in proofs {
                state.drop_proof(proof_id)?;
            }
            Ok::<_, RuntimeError>(checkpoint_state)
        })?;

        self.fee_checkpoint = Some(state);
        Ok(())
    }
}
