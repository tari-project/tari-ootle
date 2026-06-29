//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

pub struct WasmLimits {
    /// Maximum number of function arguments
    pub max_function_arguments: usize,
    /// Maximum length function names
    pub max_function_name_length: usize,
    /// Maximum number of a functions
    pub max_functions: usize,
    /// Maximum memory size in pages (64KiB each)
    pub max_memory_pages: usize,
}

pub const WASM_LIMITS: WasmLimits = WasmLimits {
    max_function_arguments: 32,
    max_function_name_length: 256,
    max_functions: 8192,
    max_memory_pages: 32, // ~2MiB = 32 * 64KiB
};

/// Maximum Wasmer metering points a single template invocation may consume. Enforced by the
/// metering middleware compiled into the engine (see `tari_engine::wasm::module::create_engine`):
/// exceeding it traps the call with an out-of-gas error.
pub const MAX_WASM_POINTS_PER_CALL: u64 = 100_000_000;

/// Maximum Wasmer metering points a whole transaction may consume, summed across every template
/// invocation it makes (top-level instructions and nested cross-template calls). Each invocation
/// otherwise gets a fresh per-call budget, so without this a transaction could multiply its
/// execution time by stacking instructions or recursing to `ENGINE_LIMITS.max_call_depth`. Enforced
/// in `WasmProcess::invoke` by capping each call's allowance to the budget remaining for the
/// transaction. Kept equal to the per-call cap: a transaction gets one compute budget, shared across
/// all its calls. The aggregate across a *block* still needs a separate per-block budget.
pub const MAX_WASM_POINTS_PER_TRANSACTION: u64 = 100_000_000;

/// Wasmer metering points a transaction may consume *before* its fee payments cover them. A
/// transaction sources its fee in the fee intent (withdraw, claim-burn, AMM swap to TARI, stealth
/// transfer, …) and only then calls `pay_fee`, so it must be allowed to run some compute on credit;
/// this bounds that credit. Beyond it, each WASM call's metering allowance is capped to the points
/// the fees paid so far can cover (`WasmProcess::invoke`), so a transaction that does not pay traps
/// out-of-gas here rather than consuming the full [`MAX_WASM_POINTS_PER_TRANSACTION`] for free. This
/// is the bound on free compute a non-paying transaction can extract from a validator. Payments
/// raise the allowance above this value proportionally to the WASM fee rate.
///
/// Sized with generous margin over the most expensive legitimate fee-sourcing flow: acquiring TARI
/// by swapping another resource through an AMM pool inside the fee intent costs ~143k points (see
/// `tari_engine`'s `complex_fee_payment` test, which guards that this stays comfortably above it),
/// so this is ~14x that worst case while staying far below the per-transaction cap.
pub const FREE_COMPUTE_GRACE_POINTS: u64 = 2_000_000;

pub struct EngineLimits {
    pub max_substate_outputs: usize,
    pub max_substate_size: usize,
    pub max_call_size: usize,
    pub max_internal_call_size: usize,
    pub max_logs: usize,
    pub max_log_size_bytes: usize,
    pub max_events: usize,
    pub max_panic_message_size: usize,
    pub max_template_binary_size_bytes: usize,
    pub max_template_name_length: usize,
    pub max_call_depth: usize,
    pub max_random_bytes_len: usize,
}

pub const ENGINE_LIMITS: EngineLimits = EngineLimits {
    max_substate_outputs: 1000,
    max_substate_size: 1024 * 1024,      // 1 MiB
    max_call_size: 128 * 1024,           // 128 KiB
    max_internal_call_size: 1024 * 1024, // 1 MiB
    max_logs: 256,
    max_log_size_bytes: 32 * 1024, // 32 KiB
    max_events: 256,
    max_panic_message_size: 32 * 1024,              // 32 KiB
    max_template_binary_size_bytes: 3 * 512 * 1024, // 1.5 MiB
    max_template_name_length: 64,
    max_call_depth: 10,
    max_random_bytes_len: 1024, // 1 KiB per call
};

pub const MAX_DIVISIBILITY: u8 = 18;

pub const MAX_TOKEN_SYMBOL_LEN: usize = 10;

/// Maximum number of `PublishTemplate` instructions a single transaction may contain.
///
/// Publishing a template registers a new global substate and carries a WASM binary up to
/// [`ENGINE_LIMITS`]`.max_template_binary_size_bytes`. Capping at one keeps each publishing transaction to a single,
/// bounded template registration; multiple publishes would stack several large binaries and their validation/storage
/// cost into one transaction with no benefit a caller cannot get from separate transactions. The engine enforces this
/// during execution — a consensus rule applied uniformly by every validator — and the mempool mirrors it to reject
/// such transactions at ingress.
pub const MAX_PUBLISH_TEMPLATES_PER_TRANSACTION: usize = 1;

pub struct StealthLimits {
    /// Maximum stealth inputs in a single transfer statement.
    pub max_inputs: usize,
    /// Maximum stealth outputs in a single transfer statement.
    pub max_outputs: usize,
    /// Maximum number of conditions in a single `SpendCondition` conjunction (TIP-0006). A revealed leaf is
    /// evaluated in full at spend time, and a builtin predicate (e.g. a covenant balance proof or a hashlock) runs
    /// native, unmetered work — so an unbounded conjunction would be a denial-of-service amplifier. This caps the
    /// worst-case work of evaluating one leaf. The condition tree itself supplies breadth (a spender reveals only one
    /// leaf plus a logarithmic inclusion proof), so the tree's size is not a spend-time cost and is not bounded here.
    pub max_conditions_per_conjunction: usize,
    /// Maximum size, in bytes, of the witness `data` blob a script-path spend may supply (`SpendWitness::ScriptPath`).
    /// The blob is processed natively by the revealed leaf's predicates, so it is bounded to cap that work. A hashlock
    /// preimage or a signature is far smaller; this leaves room for a small CBOR structure a `TemplateFunction`
    /// decodes.
    pub max_witness_data_len: usize,
    /// Maximum number of sibling hashes in a script-path inclusion proof (`MerkleProof`). The proof is
    /// spender-supplied and each sibling costs one native hash in `verify_inclusion`, so its length is bounded to
    /// keep that work constant. A proof of length `n` corresponds to a condition tree of up to `2^n` leaves, so
    /// this ceiling is far beyond any real tree (whose breadth is otherwise unbounded — see
    /// `max_conditions_per_conjunction`).
    pub max_inclusion_proof_len: usize,
    /// Maximum number of stealth transfers across a whole transaction.
    pub max_transfers_per_transaction: usize,
    /// Maximum total stealth inputs across a whole transaction.
    pub max_total_inputs_per_transaction: usize,
    /// Maximum total stealth outputs across a whole transaction.
    pub max_total_outputs_per_transaction: usize,
}

/// Verifying a stealth transfer is native, unmetered work dominated by the per-output bulletproof range proof and
/// ElGamal viewable-balance proof (~1ms per output on x86-class hardware). The per-transfer limits bound one statement;
/// the per-transaction limits bound the aggregate so a single transaction cannot stack enough stealth verification to
/// exceed the block execution budget and stall the proposing leader. The per-transaction caps are a consensus-relevant
/// execution rule enforced uniformly during execution, not just a mempool heuristic.
pub const STEALTH_LIMITS: StealthLimits = StealthLimits {
    max_inputs: 1000,
    max_outputs: 8,
    max_conditions_per_conjunction: 16,
    max_witness_data_len: 4096,
    max_inclusion_proof_len: 32,
    max_transfers_per_transaction: 64,
    max_total_inputs_per_transaction: 1024,
    max_total_outputs_per_transaction: 256,
};
