//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Balance proof signature generation and validation.

use tari_ootle_wallet_crypto::balance_proof::{
    generate_stealth_balance_proof_signature as crypto_generate,
    validate_balance_proof_signature as crypto_validate,
};
use tari_template_lib_types::{
    crypto::{BalanceProofSignature, RistrettoPublicKeyBytes, Scalar32Bytes, SchnorrSignatureBytes},
    stealth::{StealthInputsStatement, StealthOutputsStatement},
};

use crate::{error::OotleWasmError, keys::secret_key_from_bytes, sign::SchnorrSignatureResult};

/// Generate the Schnorr signature that proves
/// `∑ input_commitments + revealed_input ≡ ∑ output_commitments + revealed_output`.
///
/// `inputs_statement_json` and `outputs_statement_json` are the serialized wire-format statements. The
/// returned `(public_nonce, signature)` pair may be all-zeros for revealed-only transfers; in that case
/// callers normally omit the proof from the transfer entirely.
pub fn generate_stealth_balance_proof_signature(
    aggregated_input_mask: &[u8],
    aggregated_output_mask: &[u8],
    inputs_statement_json: &str,
    outputs_statement_json: &str,
) -> Result<SchnorrSignatureResult, OotleWasmError> {
    let agg_input_mask = secret_key_from_bytes(aggregated_input_mask)?;
    let agg_output_mask = secret_key_from_bytes(aggregated_output_mask)?;
    let inputs_statement: StealthInputsStatement = serde_json::from_str(inputs_statement_json)?;
    let outputs_statement: StealthOutputsStatement = serde_json::from_str(outputs_statement_json)?;

    let sig = crypto_generate(&agg_input_mask, &agg_output_mask, &inputs_statement, &outputs_statement);

    Ok(SchnorrSignatureResult {
        public_nonce: sig.public_nonce().as_bytes().to_vec(),
        signature: sig.signature().as_bytes().to_vec(),
    })
}

/// Verify a balance-proof signature against its inputs and outputs statements. Returns `false` on
/// malformed inputs or on an invalid signature; the engine performs the authoritative check at submission.
pub fn validate_balance_proof_signature(
    public_nonce: &[u8],
    signature: &[u8],
    inputs_statement_json: &str,
    outputs_statement_json: &str,
) -> Result<bool, OotleWasmError> {
    let public_nonce =
        RistrettoPublicKeyBytes::from_bytes(public_nonce).map_err(|e| OotleWasmError::InvalidByteLength {
            field: "public_nonce",
            expected: RistrettoPublicKeyBytes::length(),
            got: e.actual_size(),
        })?;
    let signature_scalar = Scalar32Bytes::from_bytes(signature).map_err(|e| OotleWasmError::InvalidByteLength {
        field: "signature",
        expected: Scalar32Bytes::length(),
        got: e.actual_size(),
    })?;
    let sig_bytes: BalanceProofSignature = SchnorrSignatureBytes::new(public_nonce, signature_scalar);
    let inputs_statement: StealthInputsStatement = serde_json::from_str(inputs_statement_json)?;
    let outputs_statement: StealthOutputsStatement = serde_json::from_str(outputs_statement_json)?;

    Ok(crypto_validate(&sig_bytes, &inputs_statement, &outputs_statement))
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{
        keys::{PublicKey, SecretKey},
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
        tari_utilities::ByteArray,
    };
    use tari_ootle_wallet_crypto::{
        MaskAndValue,
        OutputWitness,
        StealthInputWitness,
        StealthOutputWitness,
        stealth::create_transfer_statement,
    };
    use tari_template_lib_types::{
        Amount,
        EncryptedData,
        crypto::UtxoTag,
        stealth::{SpendCondition, StealthTransferStatement},
    };

    use super::*;

    fn build_simple_transfer() -> (RistrettoSecretKey, RistrettoSecretKey, StealthTransferStatement) {
        let mut rng = rand::rng();
        let input_mask = RistrettoSecretKey::random(&mut rng);
        let output_mask = RistrettoSecretKey::random(&mut rng);
        let owner_pk = RistrettoPublicKey::from_secret_key(&output_mask);

        let inputs = [StealthInputWitness::new(MaskAndValue::new(2000, input_mask.clone()))];
        let outputs = [StealthOutputWitness {
            witness: OutputWitness {
                amount: 2000,
                mask: output_mask.clone(),
                sender_public_nonce: owner_pk.clone(),
                minimum_value_promise: 0,
                encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                resource_view_key: None,
            },
            spend_condition: SpendCondition::Signed(owner_pk.to_byte_type()),
            tag: UtxoTag::new(0),
        }];
        let transfer = create_transfer_statement(inputs, Amount::zero(), outputs.iter(), Amount::zero()).unwrap();
        (input_mask, output_mask, transfer)
    }

    #[test]
    fn generated_signature_validates() {
        let (input_mask, output_mask, transfer) = build_simple_transfer();
        let inputs_json = serde_json::to_string(&transfer.inputs_statement).unwrap();
        let outputs_json = serde_json::to_string(&transfer.outputs_statement).unwrap();

        let sig = generate_stealth_balance_proof_signature(
            input_mask.as_bytes(),
            output_mask.as_bytes(),
            &inputs_json,
            &outputs_json,
        )
        .unwrap();

        let valid =
            validate_balance_proof_signature(&sig.public_nonce, &sig.signature, &inputs_json, &outputs_json).unwrap();
        assert!(valid);
    }

    #[test]
    fn existing_signature_validates() {
        let (_, _, transfer) = build_simple_transfer();
        let inputs_json = serde_json::to_string(&transfer.inputs_statement).unwrap();
        let outputs_json = serde_json::to_string(&transfer.outputs_statement).unwrap();
        let proof = transfer.balance_proof.unwrap();

        let valid = validate_balance_proof_signature(
            proof.public_nonce().as_bytes(),
            proof.signature().as_bytes(),
            &inputs_json,
            &outputs_json,
        )
        .unwrap();
        assert!(valid);
    }

    #[test]
    fn validate_returns_false_for_zero_signature() {
        let (_, _, transfer) = build_simple_transfer();
        let inputs_json = serde_json::to_string(&transfer.inputs_statement).unwrap();
        let outputs_json = serde_json::to_string(&transfer.outputs_statement).unwrap();
        let valid = validate_balance_proof_signature(&[0u8; 32], &[0u8; 32], &inputs_json, &outputs_json).unwrap();
        assert!(!valid);
    }

    #[test]
    fn validate_rejects_wrong_signature_length() {
        let (_, _, transfer) = build_simple_transfer();
        let inputs_json = serde_json::to_string(&transfer.inputs_statement).unwrap();
        let outputs_json = serde_json::to_string(&transfer.outputs_statement).unwrap();
        let err = validate_balance_proof_signature(&[0u8; 31], &[0u8; 32], &inputs_json, &outputs_json).unwrap_err();
        assert!(matches!(err, OotleWasmError::InvalidByteLength { .. }));
    }
}
