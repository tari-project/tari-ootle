//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::types::TemplateAddress;

use crate::runtime::{ChargeableState, StateTracker};

pub trait RuntimeModule<TStore>: Send + Sync {
    fn on_initialize(&self, _track: &mut StateTracker<TStore>) -> Result<(), RuntimeModuleError> {
        Ok(())
    }

    fn on_runtime_call(
        &self,
        _track: &mut StateTracker<TStore>,
        _call: &'static str,
    ) -> Result<(), RuntimeModuleError> {
        Ok(())
    }

    /// Invoked once for every entry into a template within the current transaction (e.g. each
    /// `call_function` / `call_method` / `update_component_template`). Modules that want to dedupe
    /// per-transaction (notably the fee module) must do so themselves via
    /// [`StateTracker::record_template_load_charge`]; the runtime intentionally does not dedupe
    /// here so observer-style modules can see every load.
    fn on_template_loaded(
        &self,
        _track: &mut StateTracker<TStore>,
        _template_address: &TemplateAddress,
        _bytes_loaded: usize,
    ) -> Result<(), RuntimeModuleError> {
        Ok(())
    }

    /// Invoked when the fee intent is checkpointed, against the state it ended on — which is the
    /// state a transaction that cannot pay for its main intent will fall back to committing.
    /// Pricing it here is what lets the paid-in-full check that follows, and the compute allowance
    /// that follows it, both account for what that fallback costs.
    ///
    /// It bounds the shortfall rather than eliminating it. Charges the main intent accrues after the
    /// last allowance was computed — a template load, the host calls made inside the final
    /// invocation — are not reserved against, so a payment sitting just above what the checkpoint
    /// required can still fall short by that much at finalization. Closing the remainder means
    /// refusing a charge that the payment cannot cover, at the point it is charged.
    fn on_fee_checkpoint(&self, _state: &mut ChargeableState<'_, TStore>) -> Result<(), RuntimeModuleError> {
        Ok(())
    }

    /// Invoked at the start of finalization, against the working state — before it is known
    /// whether the transaction commits or falls back to a fee-intent commit. Charges added here
    /// decide that outcome: they are what the paid-in-full check sees.
    fn on_before_finalize(&self, _track: &mut StateTracker<TStore>) -> Result<(), RuntimeModuleError> {
        Ok(())
    }

    /// Invoked once the state that finalization will persist has been chosen, and before its fees
    /// are settled. On a fee-intent commit that state is the fee checkpoint, not the working state
    /// [`Self::on_before_finalize`] saw, so any charge that is a function of what gets persisted
    /// must be recomputed here against `state`.
    fn on_before_persist(&self, _state: &mut ChargeableState<'_, TStore>) -> Result<(), RuntimeModuleError> {
        Ok(())
    }

    fn on_runtime_event(
        &self,
        _track: &mut StateTracker<TStore>,
        _call: &RuntimeEvent,
    ) -> Result<(), RuntimeModuleError> {
        Ok(())
    }

    /// Invoked after a WASM template invocation completes (or aborts) with the number of Wasmer
    /// metering points consumed during that call. Aggregating multiple calls within a transaction
    /// is the caller's responsibility (the runtime fans out one event per call).
    fn on_wasm_execution(
        &self,
        _track: &mut StateTracker<TStore>,
        _points_consumed: u64,
    ) -> Result<(), RuntimeModuleError> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    SignatureVerified,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeModuleError {
    #[error("BOR error: {0}")]
    Bor(#[from] tari_bor::BorError),
    #[error("Overflow error: {0}")]
    Overflow(String),
    #[error("Runtime error: {0}")]
    Runtime(String),
}
