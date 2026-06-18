//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::{EngineOp, call_engine, rust::prelude::*};
use tari_template_lib_types::{
    Amount,
    stealth::{CurrentInputView, SpendCondition, StealthInputView, StealthOutputView},
};

use crate::{
    args::{InvokeResult, SpendContextAction, SpendContextInvokeArg},
    consensus::Consensus,
};

/// The introspection handle the engine injects as the trailing argument of a spend-script predicate.
///
/// A spend script is an ordinary `is_mut == false` template function whose last parameter is a `SpendContext`. It
/// rejects a spend by **panicking** (e.g. `assert!`); returning normally authorises the spend. This matches the
/// resource `AuthHook` convention — a deliberate rejection and a script bug (panic / out-of-gas) are indistinguishable
/// by design, exactly as a failed Bitcoin script.
///
/// The accessors below are thin wrappers over the engine's read-only introspection host op
/// ([`EngineOp::SpendContextInvoke`]), scoped to the spending `StealthTransferStatement`. Confidential values are never
/// exposed — only commitments, spend conditions, `minimum_value_promise` and tags, exactly what the balance proof
/// already operates on.
#[derive(Debug, Clone, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpendContext {
    #[n(0)]
    current_input_index: u32,
}

impl SpendContext {
    /// Constructs a `SpendContext` handle. The engine calls this to inject the context; templates receive an
    /// already-constructed handle as their trailing argument and never construct one themselves.
    pub fn new(current_input_index: u32) -> Self {
        Self { current_input_index }
    }

    fn invoke(action: SpendContextAction) -> InvokeResult {
        call_engine(EngineOp::SpendContextInvoke, &SpendContextInvokeArg { action })
    }

    /// The stealth inputs being spent by this transfer (commitments only).
    pub fn inputs(&self) -> Vec<StealthInputView> {
        Self::invoke(SpendContextAction::Inputs)
            .decode()
            .expect("SpendContext::inputs returned invalid data")
    }

    /// The stealth outputs being created by this transfer, each with its commitment, `minimum_value_promise`, spend
    /// condition and tag.
    pub fn outputs(&self) -> Vec<StealthOutputView> {
        Self::invoke(SpendContextAction::Outputs)
            .decode()
            .expect("SpendContext::outputs returned invalid data")
    }

    /// The index + commitment of the input whose spend condition is currently executing.
    pub fn current_input(&self) -> CurrentInputView {
        Self::invoke(SpendContextAction::CurrentInput)
            .decode()
            .expect("SpendContext::current_input returned invalid data")
    }

    /// The local index of the input whose spend condition is currently executing.
    pub fn current_input_index(&self) -> u32 {
        self.current_input_index
    }

    /// The `SpendCondition::Script(..)` that invoked this predicate. Enables recursive covenants ("every output must
    /// carry the same condition I do").
    pub fn invoking_condition(&self) -> SpendCondition {
        Self::invoke(SpendContextAction::InvokingCondition)
            .decode()
            .expect("SpendContext::invoking_condition returned invalid data")
    }

    /// The total revealed amount being spent by this transfer.
    pub fn revealed_input_amount(&self) -> Amount {
        Self::invoke(SpendContextAction::RevealedInputAmount)
            .decode()
            .expect("SpendContext::revealed_input_amount returned invalid data")
    }

    /// The total revealed amount being output by this transfer.
    pub fn revealed_output_amount(&self) -> Amount {
        Self::invoke(SpendContextAction::RevealedOutputAmount)
            .decode()
            .expect("SpendContext::revealed_output_amount returned invalid data")
    }

    /// The current epoch, as reported by consensus. The epoch is fixed for the duration of execution.
    pub fn current_epoch(&self) -> u64 {
        Consensus::current_epoch()
    }

    // ------------------------------ Covenant helpers ------------------------------ //
    // These are thin assertions over `outputs()` / `invoking_condition()`. Authors compose them; the engine treats
    // them as ordinary predicate logic.

    /// Recursive "stay in the vault" covenant: asserts that the transfer has at least one stealth output and that every
    /// stealth output carries the same spend condition that invoked this predicate.
    ///
    /// This bounds only the spend condition of surviving stealth outputs, not the revealed amount — compose it with a
    /// revealed-output check if value must not leave the covenant as cleartext.
    pub fn require_output_preserves_condition(&self) {
        let me = self.invoking_condition();
        let outputs = self.outputs();
        assert!(
            !outputs.is_empty() && outputs.iter().all(|o| o.spend_condition == me),
            "spend script covenant: every output must preserve the invoking spend condition",
        );
    }

    /// Absolute epoch lock: asserts the current epoch is at or after `unlock_epoch`.
    pub fn require_timelock(&self, unlock_epoch: u64) {
        assert!(
            self.current_epoch() >= unlock_epoch,
            "spend script timelock: unlock epoch not reached",
        );
    }

    /// Value-flow covenant: asserts at least one output carries `spend_condition` and promises at least `min_value`.
    pub fn require_output_to(&self, spend_condition: &SpendCondition, min_value: u64) {
        assert!(
            self.outputs()
                .iter()
                .any(|o| &o.spend_condition == spend_condition && o.minimum_value_promise >= min_value),
            "spend script covenant: required output not present",
        );
    }
}
