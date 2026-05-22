//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::types::TemplateAddress;

use crate::runtime::StateTracker;

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

    fn on_before_finalize(&self, _track: &mut StateTracker<TStore>) -> Result<(), RuntimeModuleError> {
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
