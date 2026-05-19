//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Building the input side of a stealth transfer.
//!
//! The inputs statement is structurally trivial — `(commitments[], revealed_amount)` — but its on-wire
//! JSON layout is not something callers should have to know about. This module exposes a small builder
//! so Python (or any other client) can assemble a `StealthInputsStatement` from raw commitment bytes
//! without hand-crafting JSON.

use tari_crypto::{ristretto::RistrettoSecretKey, tari_utilities::ByteArray};
use tari_template_lib_types::{
    Amount,
    crypto::PedersenCommitmentBytes,
    stealth::{StealthInput, StealthInputsStatement},
};

use crate::{
    error::OotleWasmError,
    keys::{commitment_bytes_from_bytes, secret_key_from_bytes},
};

/// Build a `StealthInputsStatement` JSON from a list of raw 32-byte input commitments and a revealed
/// amount.
///
/// `input_commitments` is the concatenated bytes of all input commitments (32 bytes per commitment, so
/// the input length must be a multiple of 32). Pass an empty slice to build a revealed-only statement.
pub fn build_stealth_inputs_statement(
    input_commitments: &[u8],
    revealed_amount_microtari: u64,
) -> Result<String, OotleWasmError> {
    if !input_commitments
        .len()
        .is_multiple_of(PedersenCommitmentBytes::length())
    {
        return Err(OotleWasmError::InvalidByteLength {
            field: "input_commitments",
            expected: PedersenCommitmentBytes::length(),
            got: input_commitments.len(),
        });
    }

    let inputs = input_commitments
        .chunks_exact(PedersenCommitmentBytes::length())
        .map(|chunk| commitment_bytes_from_bytes(chunk).map(StealthInput::new))
        .collect::<Result<Vec<_>, _>>()?;

    let statement = StealthInputsStatement {
        inputs,
        revealed_amount: Amount::from_u64(revealed_amount_microtari),
    };
    Ok(serde_json::to_string(&statement)?)
}

/// Aggregate the commitment masks of stealth inputs into a single 32-byte Ristretto scalar.
///
/// `masks_concat` is the concatenated bytes of the input masks (32 bytes per mask, so the input
/// length must be a multiple of 32). Pass an empty slice to obtain the zero scalar.
///
/// Returns the sum as 32 bytes, suitable as the `aggregated_input_mask` argument to
/// [`crate::stealth::balance_proof::generate_stealth_balance_proof_signature`]. The output side of
/// the balance proof is aggregated automatically by
/// [`crate::stealth::outputs::generate_stealth_outputs_statement`].
pub fn aggregate_input_masks(masks_concat: &[u8]) -> Result<Vec<u8>, OotleWasmError> {
    const SCALAR_LEN: usize = 32;
    if !masks_concat.len().is_multiple_of(SCALAR_LEN) {
        return Err(OotleWasmError::InvalidByteLength {
            field: "masks_concat",
            expected: SCALAR_LEN,
            got: masks_concat.len(),
        });
    }

    let mut acc = RistrettoSecretKey::default();
    for chunk in masks_concat.chunks_exact(SCALAR_LEN) {
        acc = acc + secret_key_from_bytes(chunk)?;
    }
    Ok(acc.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use tari_crypto::keys::SecretKey;

    use super::*;

    #[test]
    fn build_revealed_only_statement() {
        let json = build_stealth_inputs_statement(&[], 1000).unwrap();
        let stmt: StealthInputsStatement = serde_json::from_str(&json).unwrap();
        assert!(stmt.inputs.is_empty());
        assert_eq!(stmt.revealed_amount, Amount::from_u64(1000));
    }

    #[test]
    fn build_statement_with_two_inputs() {
        let commitments: Vec<u8> = (0..64).map(|i| i as u8).collect();
        let json = build_stealth_inputs_statement(&commitments, 0).unwrap();
        let stmt: StealthInputsStatement = serde_json::from_str(&json).unwrap();
        assert_eq!(stmt.inputs.len(), 2);
        assert_eq!(stmt.inputs[0].commitment.as_bytes(), &commitments[..32]);
        assert_eq!(stmt.inputs[1].commitment.as_bytes(), &commitments[32..]);
    }

    #[test]
    fn rejects_non_multiple_of_32() {
        let err = build_stealth_inputs_statement(&[0u8; 33], 0).unwrap_err();
        assert!(matches!(err, OotleWasmError::InvalidByteLength { .. }));
    }

    #[test]
    fn aggregate_empty_returns_zero_scalar() {
        let result = aggregate_input_masks(&[]).unwrap();
        assert_eq!(result, vec![0u8; 32]);
    }

    #[test]
    fn aggregate_single_mask_returns_itself() {
        let mask = RistrettoSecretKey::random(&mut rand::rng());
        let result = aggregate_input_masks(mask.as_bytes()).unwrap();
        assert_eq!(result, mask.as_bytes().to_vec());
    }

    #[test]
    fn aggregate_two_masks_matches_native_addition() {
        let mut rng = rand::rng();
        let a = RistrettoSecretKey::random(&mut rng);
        let b = RistrettoSecretKey::random(&mut rng);
        let mut concat = a.as_bytes().to_vec();
        concat.extend_from_slice(b.as_bytes());

        let sum = aggregate_input_masks(&concat).unwrap();
        let expected = (a + b).as_bytes().to_vec();
        assert_eq!(sum, expected);
    }

    #[test]
    fn aggregate_rejects_non_multiple_of_32() {
        let err = aggregate_input_masks(&[0u8; 33]).unwrap_err();
        assert!(matches!(err, OotleWasmError::InvalidByteLength {
            field: "masks_concat",
            ..
        }));
    }
}
