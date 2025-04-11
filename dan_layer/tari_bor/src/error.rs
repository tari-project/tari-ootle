//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[cfg(not(feature = "std"))]
use alloc::{
    fmt,
    format,
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

impl From<ciborium::value::Error> for BorError {
    fn from(value: ciborium::value::Error) -> Self {
        Self(value.to_string())
    }
}
