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
use tari_template_lib_types::{
    EncryptedData,
    Hash32,
    crypto::UtxoTag,
    stealth::{SpendAuthorization, SpendWitness},
};

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
        if self.minimum_value_promise > self.amount {
            return Err(OotleWasmError::Stealth(format!(
                "minimum_value_promise ({}) must be <= amount ({})",
                self.minimum_value_promise, self.amount
            )));
        }
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

/// A stealth input being spent: the unblinded commitment opening, plus (for a script-path spend
/// only) the `SpendWitness` revealing one condition-tree leaf and the committed `condition_root`
/// it was revealed against. `witness`/`condition_root` are the exact pair `buildScriptPathWitness`
/// returns -- merge that result's `witness`/`condition_root` fields straight in alongside
/// `mask_and_value` for a script-path input. For a plain key-path spend, either omit `witness`
/// entirely or set it to `"KeyPath"`; `condition_root` must then be absent.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StealthInputWitnessJson {
    pub mask_and_value: InputWitness,
    #[serde(default)]
    pub witness: Option<SpendWitness>,
    #[serde(default)]
    pub condition_root: Option<Hash32>,
}

impl TryFrom<StealthInputWitnessJson> for StealthInputWitness {
    type Error = OotleWasmError;

    fn try_from(value: StealthInputWitnessJson) -> Result<Self, Self::Error> {
        let mask = decode_secret_key(&value.mask_and_value.mask, "mask")?;
        let mask_and_value = MaskAndValue::new(value.mask_and_value.value, mask);
        match (value.witness, value.condition_root) {
            (None | Some(SpendWitness::KeyPath), None) => Ok(StealthInputWitness::new(mask_and_value)),
            (Some(witness @ SpendWitness::ScriptPath { .. }), Some(condition_root)) => Ok(
                StealthInputWitness::with_script_path(mask_and_value, witness, condition_root),
            ),
            _ => Err(OotleWasmError::Stealth(
                "a script-path input needs both witness (\"ScriptPath\") and condition_root; a key-path input needs \
                 neither, or witness \"KeyPath\" alone"
                    .to_string(),
            )),
        }
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
    if !hex_str.len().is_multiple_of(2) {
        return Err(hex::FromHexError::OddLength.into());
    }
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

#[cfg(test)]
mod tests {
    use tari_crypto::keys::{PublicKey, SecretKey};

    use super::*;

    fn sample_output_witness_json(amount: u64, minimum_value_promise: u64) -> OutputWitnessJson {
        let mask = RistrettoSecretKey::random(&mut rand::rng());
        let nonce = RistrettoPublicKey::from_secret_key(&mask);
        OutputWitnessJson {
            amount,
            mask: hex::encode(mask.as_bytes()),
            sender_public_nonce: hex::encode(nonce.as_bytes()),
            minimum_value_promise,
            encrypted_data: EncryptedData::try_from(vec![0u8; EncryptedData::min_size()]).unwrap(),
            resource_view_key: None,
        }
    }

    #[test]
    fn try_into_witness_rejects_minimum_value_promise_above_amount() {
        let json = sample_output_witness_json(100, 101);
        let err = json.try_into_witness().unwrap_err();
        assert!(matches!(err, OotleWasmError::Stealth(ref msg) if msg.contains("minimum_value_promise")));
    }

    #[test]
    fn try_into_witness_accepts_minimum_value_promise_at_amount() {
        let json = sample_output_witness_json(100, 100);
        let witness = json.try_into_witness().unwrap();
        assert_eq!(witness.amount, 100);
        assert_eq!(witness.minimum_value_promise, 100);
    }

    #[test]
    fn decode_fixed_hex_reports_invalid_hex_on_odd_length_or_non_hex() {
        let odd_65 = format!("{}a", "00".repeat(32));
        let err = decode_secret_key(&odd_65, "mask").unwrap_err();
        assert!(matches!(err, OotleWasmError::InvalidHex(hex::FromHexError::OddLength)));

        let non_hex_64 = "zz".repeat(32);
        let err = decode_secret_key(&non_hex_64, "mask").unwrap_err();
        assert!(matches!(
            err,
            OotleWasmError::InvalidHex(hex::FromHexError::InvalidHexCharacter { .. })
        ));
    }

    #[test]
    fn decode_fixed_hex_reports_decoded_byte_length_on_mismatch() {
        let short_hex = "00".repeat(31);
        let err = decode_secret_key(&short_hex, "mask").unwrap_err();
        assert!(matches!(
            err,
            OotleWasmError::InvalidByteLength {
                field: "mask",
                expected: 32,
                got: 31,
            }
        ));
    }
}
