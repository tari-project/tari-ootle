//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::types::Signature;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Signed<T> {
    tx: T,
    signature: Signature,
}

impl<T> Signed<T> {
    pub fn new(inner: T, signature: Signature) -> Self {
        Self { tx: inner, signature }
    }

    pub fn tx(&self) -> &T {
        &self.tx
    }

    pub fn signature(&self) -> &Signature {
        &self.signature
    }

    pub fn into_parts(self) -> (T, Signature) {
        (self.tx, self.signature)
    }
}
