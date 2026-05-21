//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! ElGamal viewable-balance proof generation and decryption.

use ootle_byte_type::ConvertFromByteType;
use tari_crypto::ristretto::pedersen::PedersenCommitment;
use tari_engine_types::crypto::{
    ElgamalVerifiableBalance,
    ElgamalVerifiableBalanceBytes,
    ValueLookupTable,
    validate_elgamal_verifiable_balance_proof,
};
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

/// Decrypt a viewable balance directly from its persisted ElGamal ciphertext, recovering the bound value.
///
/// This is the read / audit path for viewable resources. It consumes the 2-field ciphertext the chain
/// actually persists in `OutputBody.viewable_balance`
/// (`ElgamalVerifiableBalanceBytes { encrypted, public_nonce }`) and needs only the view **secret** key —
/// no `ViewableBalanceProof` and no commitment. Decryption is `V.G = E - p.R`, followed by a brute-force
/// lookup of `V` over `[min_value, max_value]` (inclusive); returns `None` if no candidate matches.
///
/// Unlike [`decrypt_elgamal_viewable_balance`], this does **not** re-verify the zero-knowledge proof: that
/// proof is verified once by consensus at submission and then dropped, so a reader of a persisted UTXO
/// never has it. Re-verification is redundant for an output the chain already accepted. Use the
/// proof-based variant only when you still hold a freshly generated proof and want to verify-then-decrypt
/// before submission.
///
/// Keep the range tight: the lookup cost is proportional to its width.
pub fn decrypt_viewable_balance(
    encrypted: &[u8],
    public_nonce: &[u8],
    view_secret_key: &[u8],
    min_value: u64,
    max_value: u64,
) -> Result<Option<u64>, OotleWasmError> {
    if max_value < min_value {
        return Err(OotleWasmError::Stealth(format!(
            "max_value ({max_value}) must be >= min_value ({min_value})"
        )));
    }

    let verifiable = ElgamalVerifiableBalance {
        encrypted: public_key_from_bytes(encrypted)?,
        public_nonce: public_key_from_bytes(public_nonce)?,
    };
    let view_sk = secret_key_from_bytes(view_secret_key)?;

    // GenerateValueLookup computes `v.G` on-the-fly (no precomputed file), so this works in WASM.
    let mut lookup = GenerateValueLookup;
    verifiable
        .brute_force_balance(&view_sk, min_value..=max_value, &mut lookup)
        .map_err(|e| OotleWasmError::Stealth(format!("Value lookup failed: {e}")))
}

/// Batch variant of [`decrypt_viewable_balance`]: decrypt many persisted viewable-balance ciphertexts in
/// one call, amortising the value lookup across all of them (mirrors the engine's
/// `ElgamalVerifiableBalance::batched_brute_force`).
///
/// `ciphertexts_json` is a JSON array of persisted ciphertexts, each matching the on-chain
/// `OutputBody.viewable_balance` shape: `[{ "encrypted": "<hex32>", "public_nonce": "<hex32>" }, ...]`.
/// Every entry is decrypted with the same `view_secret_key` and `[min_value, max_value]` range.
///
/// Returns a JSON array of `number | null`, positionally aligned with the input (`null` where no value in
/// the range matched that entry), e.g. `[42, null, 1000]`.
pub fn decrypt_viewable_balance_batch(
    ciphertexts_json: &str,
    view_secret_key: &[u8],
    min_value: u64,
    max_value: u64,
) -> Result<String, OotleWasmError> {
    if max_value < min_value {
        return Err(OotleWasmError::Stealth(format!(
            "max_value ({max_value}) must be >= min_value ({min_value})"
        )));
    }

    let ciphertexts: Vec<ElgamalVerifiableBalanceBytes> = serde_json::from_str(ciphertexts_json)?;
    let view_sk = secret_key_from_bytes(view_secret_key)?;

    let verifiables = ciphertexts
        .iter()
        .map(ElgamalVerifiableBalance::convert_from_byte_type)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| OotleWasmError::InvalidPublicKey(e.to_string()))?;

    let mut lookup = GenerateValueLookup;
    let values =
        ElgamalVerifiableBalance::batched_brute_force(&view_sk, min_value..=max_value, &mut lookup, verifiables.iter())
            .map_err(|e| OotleWasmError::Stealth(format!("Value lookup failed: {e}")))?;

    Ok(serde_json::to_string(&values)?)
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

    // Persist-shaped reads: only `E` and `R` survive on-chain in `OutputBody.viewable_balance`. Derive
    // them from a freshly generated proof (the proof's 6 other fields would be dropped after submission).
    fn persisted_ciphertext(proof_json: &str) -> ViewableBalanceProof {
        serde_json::from_str(proof_json).unwrap()
    }

    #[test]
    fn decrypt_from_persisted_ciphertext_matches_full_proof_path() {
        let amount = 42u64;
        let (_mask, view_pk, view_sk, commitment, proof_json) = build_proof(amount);

        // Full-proof path (sender, pre-submission verify-then-decrypt).
        let via_proof =
            decrypt_elgamal_viewable_balance(&proof_json, &commitment, view_pk.as_bytes(), view_sk.as_bytes(), 0, 100)
                .unwrap();

        // Ciphertext-only path (reader / audit): no proof, no commitment, no view *public* key.
        let persisted = persisted_ciphertext(&proof_json);
        let via_ciphertext = decrypt_viewable_balance(
            persisted.elgamal_encrypted.as_bytes(),
            persisted.elgamal_public_nonce.as_bytes(),
            view_sk.as_bytes(),
            0,
            100,
        )
        .unwrap();

        assert_eq!(via_ciphertext, Some(amount));
        assert_eq!(via_ciphertext, via_proof);
    }

    #[test]
    fn decrypt_viewable_balance_returns_none_when_outside_range() {
        let amount = 200u64;
        let (_mask, _view_pk, view_sk, _commitment, proof_json) = build_proof(amount);
        let persisted = persisted_ciphertext(&proof_json);

        let value = decrypt_viewable_balance(
            persisted.elgamal_encrypted.as_bytes(),
            persisted.elgamal_public_nonce.as_bytes(),
            view_sk.as_bytes(),
            0,
            100,
        )
        .unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn decrypt_viewable_balance_rejects_inverted_range() {
        let (_mask, _view_pk, view_sk, _commitment, proof_json) = build_proof(0);
        let persisted = persisted_ciphertext(&proof_json);

        let err = decrypt_viewable_balance(
            persisted.elgamal_encrypted.as_bytes(),
            persisted.elgamal_public_nonce.as_bytes(),
            view_sk.as_bytes(),
            10,
            5,
        )
        .unwrap_err();
        assert!(matches!(err, OotleWasmError::Stealth(_)));
    }

    #[test]
    fn decrypt_batch_from_persisted_ciphertexts() {
        // All ciphertexts must decrypt under one shared view secret (the batch takes a single view_sk).
        let mut rng = rand::rng();
        let (view_sk, view_pk) = RistrettoPublicKey::random_keypair(&mut rng);

        let amounts = [10u64, 42, 99];
        let ciphertexts: Vec<ElgamalVerifiableBalanceBytes> = amounts
            .iter()
            .map(|&amount| {
                let mask = RistrettoSecretKey::random(&mut rng);
                let commitment = get_commitment_factory().commit_value(&mask, amount);
                let proof_json = generate_elgamal_viewable_balance_proof(
                    mask.as_bytes(),
                    amount,
                    commitment.to_byte_type().as_bytes(),
                    view_pk.as_bytes(),
                )
                .unwrap();
                let persisted = persisted_ciphertext(&proof_json);
                ElgamalVerifiableBalanceBytes {
                    encrypted: persisted.elgamal_encrypted,
                    public_nonce: persisted.elgamal_public_nonce,
                }
            })
            .collect();

        let json = serde_json::to_string(&ciphertexts).unwrap();
        let result = decrypt_viewable_balance_batch(&json, view_sk.as_bytes(), 0, 100).unwrap();
        let values: Vec<Option<u64>> = serde_json::from_str(&result).unwrap();
        assert_eq!(values, vec![Some(10), Some(42), Some(99)]);
    }

    #[test]
    fn decrypt_batch_marks_out_of_range_entries_null() {
        let mut rng = rand::rng();
        let (view_sk, view_pk) = RistrettoPublicKey::random_keypair(&mut rng);

        // 50 is inside [0, 100]; 500 is not.
        let ciphertexts: Vec<ElgamalVerifiableBalanceBytes> = [50u64, 500]
            .iter()
            .map(|&amount| {
                let mask = RistrettoSecretKey::random(&mut rng);
                let commitment = get_commitment_factory().commit_value(&mask, amount);
                let proof_json = generate_elgamal_viewable_balance_proof(
                    mask.as_bytes(),
                    amount,
                    commitment.to_byte_type().as_bytes(),
                    view_pk.as_bytes(),
                )
                .unwrap();
                let persisted = persisted_ciphertext(&proof_json);
                ElgamalVerifiableBalanceBytes {
                    encrypted: persisted.elgamal_encrypted,
                    public_nonce: persisted.elgamal_public_nonce,
                }
            })
            .collect();

        let json = serde_json::to_string(&ciphertexts).unwrap();
        let result = decrypt_viewable_balance_batch(&json, view_sk.as_bytes(), 0, 100).unwrap();
        let values: Vec<Option<u64>> = serde_json::from_str(&result).unwrap();
        assert_eq!(values, vec![Some(50), None]);
    }
}
