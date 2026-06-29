//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::{EngineOp, call_engine, rust::prelude::*};
use tari_template_lib_types::{
    Amount,
    Hash32,
    stealth::{CurrentInputView, StealthInputView, StealthOutputView, has_output_to, outputs_preserve_condition},
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

    /// The stealth outputs being created by this transfer, each with its commitment, `minimum_value_promise`, the
    /// output's authorisation (`spend_key`/`condition_root`) and tag.
    pub fn outputs(&self) -> Vec<StealthOutputView> {
        Self::invoke(SpendContextAction::Outputs)
            .decode()
            .expect("SpendContext::outputs returned invalid data")
    }

    /// The index, commitment and committed `condition_root` of the input whose condition is currently executing.
    pub fn current_input(&self) -> CurrentInputView {
        Self::invoke(SpendContextAction::CurrentInput)
            .decode()
            .expect("SpendContext::current_input returned invalid data")
    }

    /// The local index of the input whose spend condition is currently executing.
    pub fn current_input_index(&self) -> u32 {
        self.current_input_index
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

    /// The raw spender-supplied witness `data` blob for the input being spent (empty if none was provided).
    ///
    /// Unlike the predicate's committed arguments, this is uncommitted spend-time data. Decode it into the expected
    /// shape (e.g. `tari_bor::decode`) or use the raw bytes directly; the blob is bounded by
    /// `STEALTH_LIMITS.max_witness_data_len`.
    pub fn data(&self) -> Vec<u8> {
        Self::invoke(SpendContextAction::WitnessData)
            .decode()
            .expect("SpendContext::data returned invalid data")
    }

    // ------------------------------ Covenant helpers ------------------------------ //
    // These are thin assertions over `outputs()` / `invoking_condition()`. Authors compose them; the engine treats
    // them as ordinary predicate logic.
    //
    // A "partition" is every input and output of the transfer that shares the invoking input's `condition_root`.
    // Partitions are keyed by root equality, so distinct UTXOs committing the same condition tree form one partition —
    // there is no hidden per-UTXO identity. Use distinct bound args (e.g. a vault nonce) in a leaf when separate UTXOs
    // must be separate covenants.

    /// Recursive "stay in the vault" covenant: asserts that the transfer has at least one stealth output and that every
    /// stealth output is re-locked under exactly the `condition_root` committed by the UTXO being spent — a pure
    /// `Script` authorisation with no key path. An output committing the same root but adding a key path is rejected:
    /// it could be key-spent next block, escaping the covenant.
    ///
    /// This bounds only the authorisation of surviving stealth outputs, not the revealed amount — compose it with a
    /// revealed-output check if value must not leave the covenant as cleartext.
    pub fn require_output_preserves_condition(&self) {
        let view = self.current_input();
        let me = view
            .condition_root
            .as_ref()
            .expect("spend script covenant: invoking input has no condition_root");
        assert!(
            outputs_preserve_condition(&self.outputs(), me),
            "spend script covenant: every output must preserve the invoking condition_root",
        );
    }

    /// Absolute epoch lock: asserts the current epoch is at or after `unlock_epoch`.
    pub fn require_timelock(&self, unlock_epoch: u64) {
        assert!(
            self.current_epoch() >= unlock_epoch,
            "spend script timelock: unlock epoch not reached",
        );
    }

    /// Value-flow covenant: asserts at least one output is authorised by exactly `Script(condition_root)` — no key-path
    /// escape — and promises at least `min_value`. A recipient wanting a key path must encode it as a leaf in their own
    /// condition tree (keeping the output a pure `Script`), so the payment remains auditable.
    pub fn require_output_to(&self, condition_root: &Hash32, min_value: u64) {
        assert!(
            has_output_to(&self.outputs(), condition_root, min_value),
            "spend script covenant: required output not present",
        );
    }

    /// Full-conservation covenant (TIP-0006 Option A): asserts no value leaves the invoking partition. Change that
    /// stays under the invoking condition nets out, and depositing into the partition is permitted; any cleartext
    /// withdrawal of partition value is rejected. The confidential balance is never revealed.
    pub fn require_balance_preserved(&self) {
        assert!(
            self.covenant_balanced(0),
            "spend script covenant: value must be conserved within the covenant",
        );
    }

    /// Capped-withdrawal covenant (TIP-0006 Option C): asserts at most `max_revealed` cleartext leaves the invoking
    /// partition this spend. The withdrawn amount is public; the remaining confidential balance is not revealed.
    ///
    /// The allowance is per-partition, not per-UTXO: if several spent UTXOs share this exact condition they form one
    /// partition with one shared `max_revealed`. Bind a per-UTXO identity into the condition's args for per-UTXO caps.
    pub fn require_balance_preserved_with_allowance(&self, max_revealed: u64) {
        assert!(
            self.covenant_balanced(max_revealed),
            "spend script covenant: withdrawal exceeds the permitted allowance",
        );
    }

    /// Verifies the covenant sub-balance proof for the invoking partition, returning whether its value is conserved up
    /// to a cleartext outflow of at most `max_revealed`.
    fn covenant_balanced(&self, max_revealed: u64) -> bool {
        Self::invoke(SpendContextAction::AssertCovenantBalanced { max_revealed })
            .decode()
            .expect("SpendContext::covenant_balanced returned invalid data")
    }
}
