//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::{
    runtime::{RuntimeModule, RuntimeModuleError, StateTracker},
    state_store::StateReader,
};

/// Enforces the spend-script sandbox deny-list for the few effectful / non-deterministic host ops that bypass the lock
/// layer. State writes are already neutralised by the read-only chokepoint in `WorkingState::write_lock_substate` /
/// `new_substate`; this module covers the rest. This list is small and stable precisely because the lock layer already
/// covers every state write.
///
/// The module only acts while the current call frame is a read-only spend-script frame (see
/// [`StateTracker::is_in_read_only_context`]); outside that context every call is allowed, so it is safe to register
/// unconditionally.
pub struct SpendScriptGuardModule;

impl SpendScriptGuardModule {
    /// Host ops denied inside a spend-script predicate: sources of non-determinism (`generate_random_invoke`,
    /// `generate_uuid`), non-lock-mediated effects (`emit_event`, `pay_fee`, proof/bucket creation via `proof_invoke`
    /// / `bucket_invoke`), template publication, and cross-template calls (`call_invoke`). Everything else is allowed —
    /// `spend_context_invoke`, `signature_invoke`, `consensus_invoke`, `caller_context_invoke`, `emit_log`, and
    /// read-only `vault`/`resource`/`component` reads (writes amongst these are already blocked by the lock layer).
    const DENIED: &'static [&'static str] = &[
        "call_invoke",
        "generate_random_invoke",
        "generate_uuid",
        "publish_template",
        "pay_fee",
        "emit_event",
        "proof_invoke",
        "bucket_invoke",
    ];
}

impl<TStore: StateReader> RuntimeModule<TStore> for SpendScriptGuardModule {
    fn on_runtime_call(&self, track: &mut StateTracker<TStore>, call: &'static str) -> Result<(), RuntimeModuleError> {
        if track.is_in_read_only_context() && Self::DENIED.contains(&call) {
            return Err(RuntimeModuleError::Runtime(format!(
                "host operation '{call}' is not permitted inside a spend script"
            )));
        }
        Ok(())
    }
}
