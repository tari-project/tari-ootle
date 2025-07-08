//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

//! Limits for the Tari Engine WASM runtime

/// Maximum size if the name of a function
pub const MAX_FUNCTION_NAME_LENGTH: usize = 256;
/// Maximum number of function arguments
pub const MAX_FUNCTIONS_ARGUMENTS: usize = 32;
/// Maximum number of a functions
pub const MAX_FUNCTIONS: usize = 8192;
