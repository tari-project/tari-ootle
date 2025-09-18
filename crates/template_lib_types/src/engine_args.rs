//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};

use crate::crypto::{PublicKey, SignaturePayload};

// -------------------------------- Signature -------------------------------- //
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignatureAction {
    Verify,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignatureInvokeArg {
    pub action: SignatureAction,
    pub args: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignatureVerifyArgRef<'a> {
    pub public_key: &'a PublicKey,
    pub domain: &'a [u8],
    pub message: &'a [u8],
    pub payload: &'a SignaturePayload,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SignatureVerifyArg {
    pub public_key: PublicKey,
    pub domain: Vec<u8>,
    pub message: Vec<u8>,
    pub payload: SignaturePayload,
}
