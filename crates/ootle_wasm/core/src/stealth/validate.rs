//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Pre-flight validation for a full `StealthTransferStatement` envelope.

use tari_engine_types::stealth::{validate_condition_structure, validate_transfer as crypto_validate_transfer};
use tari_template_lib_types::stealth::StealthTransferStatement;

use crate::{error::OotleWasmError, keys::public_key_from_bytes};

/// Run the same validation the engine performs on a `StealthTransferStatement`: structural sanity,
/// commitment well-formedness, range and balance-proof verification.
///
/// `view_key` (optional) is the resource view-key public key — required when the resource has a viewable
/// balance enabled. Pass `None` for resources without a view key.
///
/// Returns `Ok(())` on a valid statement, or an error describing the validation failure.
pub fn validate_stealth_transfer(transfer_json: &str, view_key: Option<&[u8]>) -> Result<(), OotleWasmError> {
    let transfer: StealthTransferStatement = serde_json::from_str(transfer_json)?;
    let view_key = view_key.map(public_key_from_bytes).transpose()?;

    if !transfer.is_within_decode_limits() {
        return Err(OotleWasmError::StealthValidation(
            "Stealth transfer statement exceeds decode limits".to_string(),
        ));
    }
    for input in transfer.stealth_inputs() {
        if let Some((leaf, _, _)) = input.witness.as_script_path() {
            validate_condition_structure(leaf).map_err(|e| OotleWasmError::StealthValidation(e.to_string()))?;
        }
    }

    crypto_validate_transfer(&transfer, view_key.as_ref())
        .map_err(|e| OotleWasmError::StealthValidation(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::iter;

    use ootle_byte_type::ToByteType;
    use tari_crypto::{
        keys::{PublicKey, SecretKey},
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
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
        Hash32,
        access_rules::MAX_ACCESS_RULE_NODES,
        crypto::{PedersenCommitmentBytes, UtxoTag},
        stealth::{
            AtomicCondition,
            BuiltinPredicate,
            MAX_CONDITIONS_PER_CONJUNCTION,
            MerkleProof,
            SpendAuthorization,
            SpendCondition,
            SpendWitness,
            StealthInput,
        },
    };

    use super::*;

    #[test]
    fn validates_a_well_formed_transfer() {
        let mut rng = rand::rng();
        let input_mask = RistrettoSecretKey::random(&mut rng);
        let output_mask = RistrettoSecretKey::random(&mut rng);
        let owner_pk = RistrettoPublicKey::from_secret_key(&output_mask);

        let inputs = [StealthInputWitness::new(MaskAndValue::new(500, input_mask))];
        let outputs = [StealthOutputWitness {
            witness: OutputWitness {
                amount: 500,
                mask: output_mask,
                sender_public_nonce: owner_pk.clone(),
                minimum_value_promise: 0,
                encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                resource_view_key: None,
            },
            auth: SpendAuthorization::Key(owner_pk.to_byte_type()),
            tag: UtxoTag::new(0),
        }];
        let transfer = create_transfer_statement(inputs, Amount::zero(), outputs.iter(), None).unwrap();
        let json = serde_json::to_string(&transfer).unwrap();

        validate_stealth_transfer(&json, None).unwrap();
    }

    #[test]
    fn rejects_a_noop_transfer() {
        let transfer = create_transfer_statement(iter::empty(), Amount::zero(), iter::empty(), None).unwrap();
        let json = serde_json::to_string(&transfer).unwrap();
        let err = validate_stealth_transfer(&json, None).unwrap_err();
        assert!(matches!(err, OotleWasmError::StealthValidation(_)));
    }

    #[test]
    fn rejects_transfer_exceeding_decode_limits() {
        // Each input's leaf is within the per-conjunction bound, so JSON deserialization accepts every input; only the
        // spend condition node bound shared across all inputs is exceeded.
        let script_path_input = || {
            let leaf = SpendCondition::all(
                (0..MAX_CONDITIONS_PER_CONJUNCTION).map(|_| AtomicCondition::Builtin(BuiltinPredicate::AfterEpoch(0))),
            );
            let proof = MerkleProof::new(vec![Hash32::from_array([1; 32])]);
            StealthInput::with_witness(PedersenCommitmentBytes::zero(), SpendWitness::script_path(leaf, proof))
        };
        let mut transfer = create_transfer_statement(iter::empty(), Amount::from(10u64), iter::empty(), None).unwrap();
        transfer.inputs_statement.inputs = (0..=MAX_ACCESS_RULE_NODES / MAX_CONDITIONS_PER_CONJUNCTION)
            .map(|_| script_path_input())
            .collect();
        assert!(transfer.num_nodes() > MAX_ACCESS_RULE_NODES);
        let json = serde_json::to_string(&transfer).unwrap();
        let err = validate_stealth_transfer(&json, None).unwrap_err();
        assert!(
            matches!(&err, OotleWasmError::StealthValidation(msg) if msg.contains("decode limits")),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rejects_script_path_input_with_invalid_condition_structure() {
        let mut transfer = create_transfer_statement(iter::empty(), Amount::from(10u64), iter::empty(), None).unwrap();
        let empty_leaf = SpendCondition::all([]);
        let proof = MerkleProof::new(vec![Hash32::from_array([1; 32])]);
        transfer.inputs_statement.inputs = vec![StealthInput::with_witness(
            PedersenCommitmentBytes::zero(),
            SpendWitness::script_path(empty_leaf, proof),
        )];
        let json = serde_json::to_string(&transfer).unwrap();
        let err = validate_stealth_transfer(&json, None).unwrap_err();
        assert!(matches!(err, OotleWasmError::StealthValidation(_)));
    }
}
