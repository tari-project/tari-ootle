//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{borrow::Borrow, fmt::Display};

use tari_engine::runtime::{ActionIdent, RuntimeError};
use tari_engine_types::{
    commit_result::{ExecutionFailureCode, RejectReason},
    resource_container::ResourceError,
};

#[track_caller]
pub fn assert_reject_reason<B: Borrow<RejectReason>, E: Display>(reason: B, error: E) {
    let s = reason.borrow().to_string();
    if !s.contains(&error.to_string()) {
        panic!("Expected reject reason \"{}\" but got \"{}\"", error, s)
    }
}

/// Asserts both the rendered message and the [`ExecutionFailureCode`] a consumer would branch on.
///
/// The message pins which error was raised; the code pins how it is reported. Checking only the message
/// would let a misclassification through, and checking only the code would pass for any error sharing it.
#[track_caller]
pub fn assert_reject_reason_with_code<B: Borrow<RejectReason>, E: Display>(
    reason: B,
    error: E,
    expected: ExecutionFailureCode,
) {
    let reason = reason.borrow();
    assert_reject_reason(reason, error);
    match reason.execution_failure_code() {
        Some(code) => assert_eq!(code, expected, "wrong failure code for reject reason \"{reason}\""),
        None => panic!("Expected an execution failure with code {expected}, but got \"{reason}\""),
    }
}

#[track_caller]
pub fn assert_access_denied_for_action<B: Borrow<RejectReason>, A: Into<ActionIdent>>(reason: B, action_ident: A) {
    assert_reject_reason_with_code(
        reason,
        RuntimeError::AccessDenied {
            action_ident: action_ident.into(),
        },
        ExecutionFailureCode::AccessDenied,
    )
}

#[track_caller]
pub fn assert_insufficient_funds_for_action<B: Borrow<RejectReason>>(reason: B) {
    assert_reject_reason_with_code(
        reason,
        RuntimeError::ResourceError(ResourceError::InsufficientBalance {
            details: "Bucket contained insufficient funds".to_string(),
        }),
        ExecutionFailureCode::InsufficientFunds,
    )
}
