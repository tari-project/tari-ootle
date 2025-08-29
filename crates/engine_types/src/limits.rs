//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

pub struct WasmLimits {
    /// Maximum number of function arguments
    pub max_function_arguments: usize,
    /// Maximum length function names
    pub max_function_name_length: usize,
    /// Maximum number of a functions
    pub max_functions: usize,
}

pub const WASM_LIMITS: WasmLimits = WasmLimits {
    max_function_arguments: 32,
    max_function_name_length: 256,
    max_functions: 8192,
};

pub struct EngineLimits {
    pub max_substate_outputs: usize,
    pub max_logs: usize,
    pub max_events: usize,
}

pub const ENGINE_LIMITS: EngineLimits = EngineLimits {
    max_substate_outputs: 1000,
    max_logs: 100,
    max_events: 100,
};

pub const MAX_DIVISIBILITY: u8 = 18;

pub struct StealthLimits {
    pub max_inputs: usize,
    pub max_outputs: usize,
}

pub const STEALTH_LIMITS: StealthLimits = StealthLimits {
    max_inputs: 1000,
    max_outputs: 500,
};
