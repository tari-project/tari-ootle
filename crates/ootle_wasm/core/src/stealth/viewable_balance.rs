//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! ElGamal viewable-balance proof generation and decryption.

use ootle_byte_type::ConvertFromByteType;
use tari_crypto::ristretto::pedersen::PedersenCommitment;
use tari_engine_types::crypto::{ValueLookupTable, validate_elgamal_verifiable_balance_proof};
use tari_ootle_wallet_crypto::{
    GenerateValueLookup,
    viewable_balance_proof::generate_elgamal_viewable_balance_proof as crypto_generate,
};
use tari_template_lib_types::stealth::ViewableBalanceProof;

use crate::{
    error::OotleWasmError,
    keys::{commitment_bytes_from_bytes, public_key_from_bytes, secret_key_from_bytes},
};

/// Generate an ElGamal viewable-balance proof that the supplied `amount` is the value bound by the given
/// Pedersen `commitment`, encrypted to the resource view-key holder.
///
/// `mask` is the 32-byte commitment mask, `commitment` is the 32-byte Pedersen commitment, and
/// `view_public_key` is the resource's view public key.
///
/// Returns the JSON-encoded `ViewableBalanceProof`.
pub fn generate_elgamal_viewable_balance_proof(
    mask: &[u8],
    amount: u64,
    commitment: &[u8],
    view_public_key: &[u8],
) -> Result<String, OotleWasmError> {
    let mask = secret_key_from_bytes(mask)?;
    let commitment_bytes = commitment_bytes_from_bytes(commitment)?;
    let commitment = PedersenCommitment::convert_from_byte_type(&commitment_bytes)
        .map_err(|e| OotleWasmError::InvalidCommitment(e.to_string()))?;
    let view_key = public_key_from_bytes(view_public_key)?;

    let proof =
        crypto_generate(&mask, amount, &commitment, &view_key).map_err(|e| OotleWasmError::Stealth(e.to_string()))?;
    Ok(serde_json::to_string(&proof)?)
}

/// Brute-force decrypt an ElGamal viewable-balance proof to recover the bound value.
///
/// The decryptor checks each value in `[min_value, max_value]` (inclusive). Returns `None` if no
/// candidate value matches. This uses an on-the-fly value lookup; for large ranges callers should keep
/// the range tight to bound the work performed.
///
/// `commitment` is the Pedersen commitment the proof is bound to (the same commitment from the UTXO).
///
/// Both `view_public_key` and `view_secret_key` are required: the public key is used to re-verify the
/// ZK proof (rejecting tampered proofs before decrypting), the secret key is used for the actual ElGamal
/// decryption (`v.G = E - p.R`).
pub fn decrypt_elgamal_viewable_balance(
    proof_json: &str,
    commitment: &[u8],
    view_public_key: &[u8],
    view_secret_key: &[u8],
    min_value: u64,
    max_value: u64,
) -> Result<Option<u64>, OotleWasmError> {
    if max_value < min_value {
        return Err(OotleWasmError::Stealth(format!(
            "max_value ({max_value}) must be >= min_value ({min_value})"
        )));
    }

    let proof: ViewableBalanceProof = serde_json::from_str(proof_json)?;
    let commitment_bytes = commitment_bytes_from_bytes(commitment)?;
    let commitment = PedersenCommitment::convert_from_byte_type(&commitment_bytes)
        .map_err(|e| OotleWasmError::InvalidCommitment(e.to_string()))?;
    let view_pk = public_key_from_bytes(view_public_key)?;
    let view_sk = secret_key_from_bytes(view_secret_key)?;

    let verifiable = validate_elgamal_verifiable_balance_proof(&commitment, Some(&view_pk), Some(&proof))
        .map_err(|e| OotleWasmError::Stealth(e.to_string()))?
        .ok_or_else(|| OotleWasmError::Stealth("ElGamal proof returned no verifiable balance".to_string()))?;

    // GenerateValueLookup computes `v.G` on-the-fly — no precomputed file is required, which means this
    // works in any sandbox (including WASM with no filesystem access).
    let mut lookup: GenerateValueLookup = GenerateValueLookup;
    let result: Result<Option<u64>, <GenerateValueLookup as ValueLookupTable>::Error> =
        verifiable.brute_force_balance(&view_sk, min_value..=max_value, &mut lookup);
    let value = result.map_err(|e| OotleWasmError::Stealth(format!("Value lookup failed: {e}")))?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{
        commitment::HomomorphicCommitmentFactory,
        keys::{PublicKey, SecretKey},
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
        tari_utilities::ByteArray,
    };
    use tari_engine_types::crypto::get_commitment_factory;

    use super::*;

    fn build_proof(
        amount: u64,
    ) -> (
        RistrettoSecretKey,
        RistrettoPublicKey,
        RistrettoSecretKey,
        Vec<u8>,
        String,
    ) {
        let mut rng = rand::rng();
        let mask = RistrettoSecretKey::random(&mut rng);
        let (view_sk, view_pk) = RistrettoPublicKey::random_keypair(&mut rng);
        let commitment = get_commitment_factory().commit_value(&mask, amount);
        let commitment_bytes = commitment.to_byte_type();
        let proof_json = generate_elgamal_viewable_balance_proof(
            mask.as_bytes(),
            amount,
            commitment_bytes.as_bytes(),
            view_pk.as_bytes(),
        )
        .unwrap();
        (mask, view_pk, view_sk, commitment_bytes.as_bytes().to_vec(), proof_json)
    }

    #[test]
    fn round_trip_with_view_key_holder() {
        let amount = 42u64;
        let (_mask, view_pk, view_sk, commitment, proof_json) = build_proof(amount);

        let decoded =
            decrypt_elgamal_viewable_balance(&proof_json, &commitment, view_pk.as_bytes(), view_sk.as_bytes(), 0, 100)
                .unwrap();
        assert_eq!(decoded, Some(amount));
    }

    #[test]
    fn returns_none_when_value_is_outside_range() {
        let amount = 200u64;
        let (_mask, view_pk, view_sk, commitment, proof_json) = build_proof(amount);

        let decoded =
            decrypt_elgamal_viewable_balance(&proof_json, &commitment, view_pk.as_bytes(), view_sk.as_bytes(), 0, 100)
                .unwrap();
        assert_eq!(decoded, None);
    }

    #[test]
    fn rejects_inverted_range() {
        let (_mask, view_pk, view_sk, commitment, proof_json) = build_proof(0);
        let err =
            decrypt_elgamal_viewable_balance(&proof_json, &commitment, view_pk.as_bytes(), view_sk.as_bytes(), 10, 5)
                .unwrap_err();
        assert!(matches!(err, OotleWasmError::Stealth(_)));
    }
}
