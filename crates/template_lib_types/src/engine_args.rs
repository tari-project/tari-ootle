//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::rust::prelude::*;

use crate::{
    bytes::Bytes,
    crypto::{PublicKey, SignaturePayload},
};

// -------------------------------- Signature -------------------------------- //
#[derive(Debug, Clone, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SignatureAction {
    #[n(0)]
    Verify,
}

#[derive(Clone, Debug, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SignatureInvokeArg {
    #[n(0)]
    pub action: SignatureAction,
    #[n(1)]
    pub args: Vec<Bytes>,
}

#[derive(Debug, Clone, Encode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SignatureVerifyArgRef<'a> {
    #[n(0)]
    pub public_key: &'a PublicKey,
    #[n(1)]
    #[cfg_attr(feature = "serde", serde(serialize_with = "crate::bytes::serialize"))]
    #[cbor(with = "minicbor::bytes")]
    pub domain: &'a [u8],
    #[n(2)]
    #[cfg_attr(feature = "serde", serde(serialize_with = "crate::bytes::serialize"))]
    #[cbor(with = "minicbor::bytes")]
    pub message: &'a [u8],
    #[n(3)]
    pub payload: &'a SignaturePayload,
}

#[derive(Debug, Clone, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct SignatureVerifyArg {
    #[n(0)]
    pub public_key: PublicKey,
    #[n(1)]
    pub domain: Bytes,
    #[n(2)]
    pub message: Bytes,
    #[n(3)]
    pub payload: SignaturePayload,
}
