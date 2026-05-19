//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Building the output side of a stealth transfer: per-output commitments + encrypted data, optional
//! ElGamal view-key proofs, and the aggregated bulletproof range proof.

use tari_crypto::{ristretto::RistrettoSecretKey, tari_utilities::ByteArray};
use tari_ootle_wallet_crypto::{
    bullet_proof::generate_extended_bullet_proof as crypto_generate_extended_bullet_proof,
    stealth::create_outputs_statement,
};
use tari_template_lib_types::{Amount, crypto::RangeProofBytes};

use crate::{
    error::OotleWasmError,
    stealth::types::{OutputWitnessJson, StealthOutputWitnessJson},
};

/// Result of [`generate_stealth_outputs_statement`]: the wire-ready statement plus the aggregated mask the
/// sender retains for the balance proof.
#[derive(Debug, Clone)]
pub struct StealthOutputsResult {
    /// Serialized `StealthOutputsStatement` (containing outputs, revealed amount, and aggregated range
    /// proof).
    pub statement_json: String,
    /// Sum of all witness masks as a 32-byte Ristretto scalar. Feed this into
    /// [`crate::stealth::balance_proof::generate_stealth_balance_proof_signature`] as the aggregated
    /// output mask.
    pub aggregated_output_mask: Vec<u8>,
}

/// Generate the output half of a stealth transfer.
///
/// `witnesses_json` is a JSON array of [`StealthOutputWitnessJson`]; `revealed_output_amount_microtari` is
/// the plaintext revealed amount (in microtari, fits a `u64`). Output witnesses that contain a
/// `resource_view_key` automatically receive an ElGamal viewable-balance proof.
pub fn generate_stealth_outputs_statement(
    witnesses_json: &str,
    revealed_output_amount_microtari: u64,
) -> Result<StealthOutputsResult, OotleWasmError> {
    use tari_ootle_wallet_crypto::StealthOutputWitness;

    let witnesses: Vec<StealthOutputWitnessJson> = serde_json::from_str(witnesses_json)?;
    let witnesses: Vec<StealthOutputWitness> = witnesses
        .into_iter()
        .map(StealthOutputWitness::try_from)
        .collect::<Result<Vec<_>, OotleWasmError>>()?;

    let aggregated_output_mask = witnesses
        .iter()
        .map(|w| &w.witness.mask)
        .fold(RistrettoSecretKey::default(), |acc, mask| acc + mask);

    let statement = create_outputs_statement(witnesses.iter(), Amount::from_u64(revealed_output_amount_microtari))
        .map_err(|e| OotleWasmError::Stealth(e.to_string()))?;

    Ok(StealthOutputsResult {
        statement_json: serde_json::to_string(&statement)?,
        aggregated_output_mask: aggregated_output_mask.as_bytes().to_vec(),
    })
}

/// Generate an extended bulletproof that aggregates range proofs for the supplied output witnesses.
///
/// Exposed independently of [`generate_stealth_outputs_statement`] for testing and audit. The Python
/// transfer flow uses the bundled variant.
///
/// `witnesses_json` is a JSON array of [`OutputWitnessJson`]. Returns the raw `RangeProofBytes` (may be
/// empty for an empty input array).
pub fn generate_extended_bullet_proof(witnesses_json: &str) -> Result<Vec<u8>, OotleWasmError> {
    let witnesses: Vec<OutputWitnessJson> = serde_json::from_str(witnesses_json)?;
    let witnesses = witnesses
        .into_iter()
        .map(OutputWitnessJson::try_into_witness)
        .collect::<Result<Vec<_>, OotleWasmError>>()?;

    let proof: RangeProofBytes =
        crypto_generate_extended_bullet_proof(witnesses.iter()).map_err(|e| OotleWasmError::Stealth(e.to_string()))?;
    Ok(proof.into_vec())
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{
        keys::{PublicKey, SecretKey},
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    };
    use tari_engine_types::stealth::validate_stealth_outputs_statement;
    use tari_template_lib_types::{EncryptedData, stealth::StealthOutputsStatement};

    use super::*;

    fn make_witness_json(amount: u64) -> String {
        let mask = RistrettoSecretKey::random(&mut rand::rng());
        let nonce = RistrettoPublicKey::from_secret_key(&mask);
        let spend_pk: tari_template_lib_types::crypto::RistrettoPublicKeyBytes = nonce.to_byte_type();
        format!(
            r#"[{{"witness":{{"amount":{},"mask":"{}","sender_public_nonce":"{}","minimum_value_promise":0,"encrypted_data":"{}"}},"spend_condition":{{"Signed":"{}"}},"tag":0}}]"#,
            amount,
            hex::encode(mask.as_bytes()),
            hex::encode(nonce.as_bytes()),
            hex::encode(vec![0u8; EncryptedData::min_size()]),
            hex::encode(spend_pk.as_bytes()),
        )
    }

    #[test]
    fn generate_outputs_produces_valid_statement() {
        let witnesses = make_witness_json(1000);
        let result = generate_stealth_outputs_statement(&witnesses, 0).unwrap();
        let stmt: StealthOutputsStatement = serde_json::from_str(&result.statement_json).unwrap();
        validate_stealth_outputs_statement(&stmt, None).unwrap();
        assert_eq!(result.aggregated_output_mask.len(), 32);
    }

    #[test]
    fn generate_outputs_with_empty_array() {
        let result = generate_stealth_outputs_statement("[]", 100).unwrap();
        let stmt: StealthOutputsStatement = serde_json::from_str(&result.statement_json).unwrap();
        assert!(stmt.outputs.is_empty());
        assert!(stmt.agg_range_proof.is_empty());
        assert_eq!(result.aggregated_output_mask, vec![0u8; 32]);
    }

    #[test]
    fn generate_extended_bullet_proof_empty() {
        let proof = generate_extended_bullet_proof("[]").unwrap();
        assert!(proof.is_empty());
    }

    #[test]
    fn generate_extended_bullet_proof_non_empty() {
        // Flat OutputWitnessJson (no spend_condition / tag wrapper).
        let mask = RistrettoSecretKey::random(&mut rand::rng());
        let nonce = RistrettoPublicKey::from_secret_key(&mask);
        let witness_json = format!(
            r#"[{{"amount":50,"mask":"{}","sender_public_nonce":"{}","minimum_value_promise":0,"encrypted_data":"{}"}}]"#,
            hex::encode(mask.as_bytes()),
            hex::encode(nonce.as_bytes()),
            hex::encode(vec![0u8; EncryptedData::min_size()]),
        );
        let proof = generate_extended_bullet_proof(&witness_json).unwrap();
        assert!(!proof.is_empty());
    }
}
