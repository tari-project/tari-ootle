//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Error variants used in the template library. These are used as error messages in panics and as error codes
//! and reduce WASM binary size by avoiding the including long error messages in the binary.

/// The engine fails to decode a value
pub const ERR_ENGINE_DECODE_FAIL: &str = "EngDcdFail";
/// A function that requires a component context is called outside of a component context
pub const ERR_NOT_IN_COMPONENT_CONTEXT: &str = "NotInCpntCtx";
/// The auth hook function name exceeds the maximum length
pub const ERR_AUTH_HOOK_FN_NAME_LEN: &str = "AuthHookFnNameLen";
