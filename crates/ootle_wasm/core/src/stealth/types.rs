//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! JSON wrapper types for stealth witnesses.
//!
//! The internal `OutputWitness`, `StealthOutputWitness` and `StealthInputWitness` types in
//! `tari_ootle_wallet_crypto` are intentionally not `Serialize`/`Deserialize`. These mirror types provide a
//! stable JSON shape for the WASM ABI and convert into the internal types via [`Into`].
//!
//! All 32-byte values (masks, public keys) are hex-encoded strings; `encrypted_data` follows the standard
//! `EncryptedData` hex encoding; `auth` and `tag` are the same shapes used by the wallet daemon
//! RPC layer.

use serde::{Deserialize, Serialize};
use tari_crypto::{
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArray,
};
use tari_ootle_wallet_crypto::{MaskAndValue, OutputWitness, StealthInputWitness, StealthOutputWitness};
use tari_template_lib_types::{EncryptedData, crypto::UtxoTag, stealth::SpendAuthorization};

use crate::{
    error::OotleWasmError,
    keys::{public_key_from_bytes, secret_key_from_bytes},
};

/// Unblinded data for a single stealth output (sender side).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputWitnessJson {
    pub amount: u64,
    /// Hex-encoded 32-byte Ristretto scalar.
    pub mask: String,
    /// Hex-encoded 32-byte Ristretto public key.
    pub sender_public_nonce: String,
    #[serde(default)]
    pub minimum_value_promise: u64,
    pub encrypted_data: EncryptedData,
    /// Optional hex-encoded 32-byte Ristretto public key for the resource view-key holder.
    #[serde(default)]
    pub resource_view_key: Option<String>,
}

impl OutputWitnessJson {
    pub(crate) fn try_into_witness(self) -> Result<OutputWitness, OotleWasmError> {
        let mask = decode_secret_key(&self.mask, "mask")?;
        let sender_public_nonce = decode_public_key(&self.sender_public_nonce, "sender_public_nonce")?;
        let resource_view_key = self
            .resource_view_key
            .as_deref()
            .map(|h| decode_public_key(h, "resource_view_key"))
            .transpose()?;
        Ok(OutputWitness {
            amount: self.amount,
            mask,
            sender_public_nonce,
            minimum_value_promise: self.minimum_value_promise,
            encrypted_data: self.encrypted_data,
            resource_view_key,
        })
    }
}

impl From<&OutputWitness> for OutputWitnessJson {
    fn from(witness: &OutputWitness) -> Self {
        // Destructured so adding a field to `OutputWitness` is a compile error here, keeping this in
        // lock-step with the inverse `try_into_witness`.
        let OutputWitness {
            amount,
            mask,
            sender_public_nonce,
            minimum_value_promise,
            encrypted_data,
            resource_view_key,
        } = witness;
        Self {
            amount: *amount,
            mask: hex::encode(mask.as_bytes()),
            sender_public_nonce: hex::encode(sender_public_nonce.as_bytes()),
            minimum_value_promise: *minimum_value_promise,
            encrypted_data: encrypted_data.clone(),
            resource_view_key: resource_view_key.as_ref().map(|k| hex::encode(k.as_bytes())),
        }
    }
}

/// A full stealth output witness: the unblinded data plus the output's [`SpendAuthorization`] and UTXO tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StealthOutputWitnessJson {
    pub witness: OutputWitnessJson,
    pub auth: SpendAuthorization,
    pub tag: UtxoTag,
}

impl From<&StealthOutputWitness> for StealthOutputWitnessJson {
    fn from(witness: &StealthOutputWitness) -> Self {
        Self {
            witness: OutputWitnessJson::from(&witness.witness),
            auth: witness.auth.clone(),
            tag: witness.tag,
        }
    }
}

impl TryFrom<StealthOutputWitnessJson> for StealthOutputWitness {
    type Error = OotleWasmError;

    fn try_from(value: StealthOutputWitnessJson) -> Result<Self, Self::Error> {
        Ok(StealthOutputWitness {
            witness: value.witness.try_into_witness()?,
            auth: value.auth,
            tag: value.tag,
        })
    }
}

/// An input being spent: the plaintext value and the commitment mask used to bind it. Once decrypted, the
/// receiver has both halves and can construct this directly.
#[derive(Debug, Clone, Deserialize)]
pub struct InputWitness {
    pub value: u64,
    /// Hex-encoded 32-byte Ristretto scalar.
    pub mask: String,
}

/// A stealth input being spent (currently just the unblinded commitment opening).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum StealthInputWitnessJson {
    /// Object form: `{ "mask_and_value": { "value": 100, "mask": "..." } }`.
    Wrapped { mask_and_value: InputWitness },
    /// Flat form: `{ "value": 100, "mask": "..." }`.
    Flat(InputWitness),
}

impl StealthInputWitnessJson {
    fn into_inner(self) -> InputWitness {
        match self {
            Self::Wrapped { mask_and_value } => mask_and_value,
            Self::Flat(w) => w,
        }
    }
}

impl TryFrom<StealthInputWitnessJson> for StealthInputWitness {
    type Error = OotleWasmError;

    fn try_from(value: StealthInputWitnessJson) -> Result<Self, Self::Error> {
        let inner = value.into_inner();
        let mask = decode_secret_key(&inner.mask, "mask")?;
        Ok(StealthInputWitness::new(MaskAndValue::new(inner.value, mask)))
    }
}

pub(crate) fn decode_secret_key(hex_str: &str, field: &'static str) -> Result<RistrettoSecretKey, OotleWasmError> {
    let bytes = decode_fixed_hex::<32>(hex_str, field)?;
    secret_key_from_bytes(&bytes)
}

pub(crate) fn decode_public_key(hex_str: &str, field: &'static str) -> Result<RistrettoPublicKey, OotleWasmError> {
    let bytes = decode_fixed_hex::<32>(hex_str, field)?;
    public_key_from_bytes(&bytes)
}

fn decode_fixed_hex<const N: usize>(hex_str: &str, field: &'static str) -> Result<[u8; N], OotleWasmError> {
    if hex_str.len() != N * 2 {
        return Err(OotleWasmError::InvalidByteLength {
            field,
            expected: N,
            got: hex_str.len() / 2,
        });
    }
    let mut out = [0u8; N];
    hex::decode_to_slice(hex_str, &mut out)?;
    Ok(out)
}
