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

pub struct StealthLimits {
    pub max_inputs: usize,
    pub max_outputs: usize,
    /// Maximum number of conditions in a single `SpendCondition::All` conjunction (TIP-0006). A revealed leaf is
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
}

pub const STEALTH_LIMITS: StealthLimits = StealthLimits {
    max_inputs: 1000,
    max_outputs: 8,
    max_conditions_per_conjunction: 16,
    max_witness_data_len: 4096,
};
