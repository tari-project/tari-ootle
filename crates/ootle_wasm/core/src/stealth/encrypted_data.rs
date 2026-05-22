//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Decrypting inbound stealth UTXOs.

use tari_crypto::tari_utilities::ByteArray;
use tari_ootle_wallet_crypto::encrypted_data::unblind_output as crypto_unblind_output;
use tari_template_lib_types::EncryptedData;

use crate::{
    error::OotleWasmError,
    keys::{commitment_bytes_from_bytes, secret_key_from_bytes},
};

/// Result of decrypting and verifying an inbound stealth UTXO payload.
#[derive(Debug, Clone)]
pub struct DecryptedOutputResult {
    /// The 32-byte commitment mask scalar.
    pub mask: Vec<u8>,
    /// The plaintext value (microtari).
    pub value: u64,
    /// JSON-encoded `Memo` (one of `U256`/`Message`/`Bytes`/`PayRefAndBytes`) or `None` if the encrypted
    /// payload carried no memo, or if the caller passed `skip_memo`.
    pub memo_json: Option<String>,
}

/// Decrypt an inbound stealth UTXO's `encrypted_data` blob and verify that the recovered (mask, value)
/// pair commits to the same Pedersen commitment.
///
/// `encrypted_data_bytes` is the raw encrypted blob (between [`EncryptedData::min_size`] and
/// [`EncryptedData::max_size`] bytes, including AEAD tag, nonce and ciphertext).
///
/// Returns `Err` on AEAD failure or a commitment mismatch — both indicate the payload was not produced
/// for this view key.
pub fn unblind_output(
    output_commitment: &[u8],
    encrypted_data_bytes: &[u8],
    encryption_key: &[u8],
    skip_memo: bool,
) -> Result<DecryptedOutputResult, OotleWasmError> {
    let commitment = commitment_bytes_from_bytes(output_commitment)?;
    let encrypted_data = EncryptedData::try_from(encrypted_data_bytes.to_vec()).map_err(|len| {
        OotleWasmError::InvalidEncryptedData(format!(
            "invalid length {len}; expected [{}, {}] bytes",
            EncryptedData::min_size(),
            EncryptedData::max_size()
        ))
    })?;
    let encryption_key = secret_key_from_bytes(encryption_key)?;

    let decrypted = crypto_unblind_output(&commitment, &encrypted_data, &encryption_key, skip_memo)
        .map_err(|e| OotleWasmError::Stealth(e.to_string()))?;

    let memo_json = decrypted.memo().map(serde_json::to_string).transpose()?;

    Ok(DecryptedOutputResult {
        mask: decrypted.mask().as_bytes().to_vec(),
        value: decrypted.value(),
        memo_json,
    })
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{commitment::HomomorphicCommitmentFactory, keys::SecretKey, ristretto::RistrettoSecretKey};
    use tari_engine_types::crypto::get_commitment_factory;
    use tari_ootle_wallet_crypto::{encrypted_data::encrypt_data, memo::Memo};

    use super::*;

    #[test]
    fn unblind_round_trip_without_memo() {
        let encryption_key = RistrettoSecretKey::random(&mut rand::rng());
        let mask = RistrettoSecretKey::random(&mut rand::rng());
        let amount = 42u64;
        let commitment = get_commitment_factory().commit_value(&mask, amount).to_byte_type();
        let encrypted = encrypt_data(amount, &mask, &encryption_key, None).unwrap();

        let result = unblind_output(
            commitment.as_bytes(),
            encrypted.as_bytes(),
            encryption_key.as_bytes(),
            false,
        )
        .unwrap();
        assert_eq!(result.value, amount);
        assert_eq!(result.mask, mask.as_bytes());
        assert!(result.memo_json.is_none());
    }

    #[test]
    fn unblind_round_trip_with_memo() {
        let encryption_key = RistrettoSecretKey::random(&mut rand::rng());
        let mask = RistrettoSecretKey::random(&mut rand::rng());
        let amount = 7u64;
        let commitment = get_commitment_factory().commit_value(&mask, amount).to_byte_type();
        let memo = Memo::new_message("hi").unwrap();
        let encrypted = encrypt_data(amount, &mask, &encryption_key, Some(&memo)).unwrap();

        let result = unblind_output(
            commitment.as_bytes(),
            encrypted.as_bytes(),
            encryption_key.as_bytes(),
            false,
        )
        .unwrap();
        let memo_json = result.memo_json.expect("memo should be decoded");
        let decoded: Memo = serde_json::from_str(&memo_json).unwrap();
        assert_eq!(decoded, memo);

        // skip_memo discards the memo even if present
        let result = unblind_output(
            commitment.as_bytes(),
            encrypted.as_bytes(),
            encryption_key.as_bytes(),
            true,
        )
        .unwrap();
        assert!(result.memo_json.is_none());
    }

    #[test]
    fn unblind_rejects_wrong_key() {
        let encryption_key = RistrettoSecretKey::random(&mut rand::rng());
        let other_key = RistrettoSecretKey::random(&mut rand::rng());
        let mask = RistrettoSecretKey::random(&mut rand::rng());
        let amount = 1u64;
        let commitment = get_commitment_factory().commit_value(&mask, amount).to_byte_type();
        let encrypted = encrypt_data(amount, &mask, &encryption_key, None).unwrap();

        let err = unblind_output(commitment.as_bytes(), encrypted.as_bytes(), other_key.as_bytes(), false).unwrap_err();
        assert!(matches!(err, OotleWasmError::Stealth(_)));
    }
}
