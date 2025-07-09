//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub struct EngineLimits {
    pub max_call_args: usize,
    // TODO: restrict the maximum length of function names
    // pub max_function_name_length: usize,
}

pub const ENGINE_LIMITS: EngineLimits = EngineLimits {
    max_call_args: 100,
    // max_function_name_length: 64,
};

pub const MAX_DIVISIBILITY: u8 = 18;
