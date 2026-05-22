//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[cfg(not(feature = "std"))]
use alloc::{
    fmt,
    string::{String, ToString},
};
#[cfg(feature = "std")]
use std::fmt;

#[derive(Debug)]
pub struct BorError(String);

impl BorError {
    pub fn new(str: String) -> Self {
        Self(str)
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for BorError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for BorError {}

impl From<minicbor::decode::Error> for BorError {
    fn from(value: minicbor::decode::Error) -> Self {
        Self(value.to_string())
    }
}

impl<E: fmt::Display> From<minicbor::encode::Error<E>> for BorError {
    fn from(value: minicbor::encode::Error<E>) -> Self {
        Self(value.to_string())
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for BorError {
    fn from(value: std::io::Error) -> Self {
        Self(value.to_string())
    }
}
