//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::convert::Infallible;

/// A trait for responses that can provide a [ResponseErrorStatus]
pub trait TransactionStatusResponseError {
    fn get_status(&self) -> ResponseErrorStatus;
    fn get_error_message(&self) -> String;
}

// This is required for tests (PanicInterface) - in general, if a type is `Infallible` it should never reach the error.
impl TransactionStatusResponseError for Infallible {
    fn get_status(&self) -> ResponseErrorStatus {
        unreachable!("Infallible is not constructible, therefore this is unreachable")
    }

    fn get_error_message(&self) -> String {
        unreachable!("Infallible is not constructible, therefore this is unreachable")
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ResponseErrorStatus {
    #[error("Not found: {message}")]
    NotFound { message: String },
    #[error("Transaction rejected: {message}")]
    TransactionRejected { message: String },
    #[error("Internal error: {message}")]
    InternalError { message: String },
}

impl TransactionStatusResponseError for ResponseErrorStatus {
    fn get_status(&self) -> ResponseErrorStatus {
        self.clone()
    }

    fn get_error_message(&self) -> String {
        self.to_string()
    }
}
