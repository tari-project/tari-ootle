//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};

use crate::{
    bytes::Bytes,
    crypto::{PublicKey, SignaturePayload},
};

// -------------------------------- Signature -------------------------------- //
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignatureAction {
    Verify,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignatureInvokeArg {
    pub action: SignatureAction,
    pub args: Vec<Bytes>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignatureVerifyArgRef<'a> {
    pub public_key: &'a PublicKey,
    #[serde(serialize_with = "crate::bytes::serialize")]
    pub domain: &'a [u8],
    #[serde(serialize_with = "crate::bytes::serialize")]
    pub message: &'a [u8],
    pub payload: &'a SignaturePayload,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SignatureVerifyArg {
    pub public_key: PublicKey,
    pub domain: Bytes,
    pub message: Bytes,
    pub payload: SignaturePayload,
}
