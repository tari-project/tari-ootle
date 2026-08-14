//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Building a complete stealth transfer statement (inputs statement, outputs statement, balance
//! proof, and covenant claims) from unblinded input/output witnesses.

use tari_ootle_wallet_crypto::stealth::create_transfer_statement as crypto_create_transfer_statement;
use tari_template_lib_types::Amount;

use crate::{
    error::OotleWasmError,
    stealth::types::{StealthInputWitnessJson, StealthOutputWitnessJson},
};

/// Build a complete `StealthTransferStatement` JSON from unblinded input/output witnesses.
///
/// Unlike `generateStealthOutputsStatement` / `buildStealthInputsStatement(FromInputs)` +
/// `generateStealthBalanceProofSignature`, which build and sign each half of a transfer
/// separately and never populate `covenant_claims`, this wraps `create_transfer_statement`
/// directly -- the single primitive that produces the *entire*, internally-consistent statement,
/// including a real covenant balance-integrity proof for any script-path-spent input's
/// condition-root partition. **This is the only correct way to spend a `PayTo::Conditions`
/// (ScriptPath) stealth output** -- the separate-calls path above has no way to populate
/// `covenant_claims` at all, and omitting a required claim there is a balance-integrity gap, not
/// a cosmetic one.
///
/// `input_witnesses_json` / `output_witnesses_json` are JSON arrays of
/// [`StealthInputWitnessJson`] / [`StealthOutputWitnessJson`] (see `stealth::types`) -- each
/// input witness carries its own `witness`/`condition_root` pair (from `buildScriptPathWitness`)
/// for a script-path spend, or neither for a plain key-path spend. A statement mixing key-path
/// and script-path inputs in the same transfer is fully supported -- each input's witness is
/// independent.
pub fn build_stealth_transfer_statement(
    input_witnesses_json: &str,
    revealed_input_amount_microtari: u64,
    output_witnesses_json: &str,
    revealed_output_amount_microtari: u64,
) -> Result<String, OotleWasmError> {
    let inputs: Vec<StealthInputWitnessJson> = serde_json::from_str(input_witnesses_json)?;
    let inputs = inputs
        .into_iter()
        .map(TryInto::try_into)
        .collect::<Result<Vec<_>, OotleWasmError>>()?;

    let outputs: Vec<StealthOutputWitnessJson> = serde_json::from_str(output_witnesses_json)?;
    let outputs = outputs
        .into_iter()
        .map(TryInto::try_into)
        .collect::<Result<Vec<_>, OotleWasmError>>()?;

    let statement = crypto_create_transfer_statement(
        inputs,
        Amount::from_u64(revealed_input_amount_microtari),
        outputs.iter(),
        Amount::from_u64(revealed_output_amount_microtari),
    )
    .map_err(|e| OotleWasmError::Stealth(e.to_string()))?;

    Ok(serde_json::to_string(&statement)?)
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{
        keys::{PublicKey, SecretKey},
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
        tari_utilities::ByteArray,
    };
    use tari_template_lib_types::{
        EncryptedData,
        crypto::UtxoTag,
        stealth::{
            AtomicCondition,
            BuiltinPredicate,
            HashAlg,
            SpendAuthorization,
            SpendCondition,
            StealthTransferStatement,
        },
    };

    use super::*;
    use crate::stealth::inputs::build_script_path_witness;

    fn output_witness_json(amount: u64, mask: &RistrettoSecretKey) -> String {
        let owner_pk = RistrettoPublicKey::from_secret_key(mask);
        serde_json::to_string(&StealthOutputWitnessJson {
            witness: crate::stealth::types::OutputWitnessJson {
                amount,
                mask: hex::encode(mask.as_bytes()),
                sender_public_nonce: hex::encode(owner_pk.as_bytes()),
                minimum_value_promise: 0,
                encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                resource_view_key: None,
            },
            auth: SpendAuthorization::Key(owner_pk.to_byte_type()),
            tag: UtxoTag::new(0),
        })
        .unwrap()
    }

    #[test]
    fn key_path_only_round_trips_through_validation() {
        let input_mask = RistrettoSecretKey::random(&mut rand::rng());
        let output_mask = RistrettoSecretKey::random(&mut rand::rng());

        let inputs_json = format!(
            r#"[{{"mask_and_value":{{"value":500,"mask":"{}"}}}}]"#,
            hex::encode(input_mask.as_bytes())
        );
        let outputs_json = format!("[{}]", output_witness_json(500, &output_mask));

        let statement_json = build_stealth_transfer_statement(&inputs_json, 0, &outputs_json, 0).unwrap();
        let statement: StealthTransferStatement = serde_json::from_str(&statement_json).unwrap();
        assert!(statement.balance_proof.is_some());
        assert!(
            statement.covenant_claims.is_empty(),
            "a key-path-only transfer needs no covenant claims"
        );

        crate::stealth::validate::validate_stealth_transfer(&statement_json, None).unwrap();
    }

    #[test]
    fn script_path_input_produces_a_real_covenant_claim_and_validates() {
        let hash = [7u8; 32];
        let conditions = vec![
            SpendCondition::all([AtomicCondition::Builtin(BuiltinPredicate::HashLock {
                hash: hash.into(),
                alg: HashAlg::Sha256,
            })]),
            SpendCondition::all([AtomicCondition::Builtin(BuiltinPredicate::AfterEpoch(1000))]),
        ];
        let conditions_json = serde_json::to_string(&conditions).unwrap();
        let claim_leaf_json = serde_json::to_string(&conditions[0]).unwrap();

        let witness_result_json = build_script_path_witness(&conditions_json, &claim_leaf_json, &[]).unwrap();
        let witness_result: serde_json::Value = serde_json::from_str(&witness_result_json).unwrap();

        let input_mask = RistrettoSecretKey::random(&mut rand::rng());
        let output_mask = RistrettoSecretKey::random(&mut rand::rng());
        let inputs_json = format!(
            r#"[{{"mask_and_value":{{"value":500,"mask":"{}"}},"witness":{},"condition_root":{}}}]"#,
            hex::encode(input_mask.as_bytes()),
            witness_result["witness"],
            witness_result["condition_root"]
        );
        let outputs_json = format!("[{}]", output_witness_json(500, &output_mask));

        let statement_json = build_stealth_transfer_statement(&inputs_json, 0, &outputs_json, 0).unwrap();
        let statement: StealthTransferStatement = serde_json::from_str(&statement_json).unwrap();
        assert_eq!(
            statement.covenant_claims.len(),
            1,
            "spending a script-path input must produce exactly one covenant claim for its condition-root partition"
        );

        crate::stealth::validate::validate_stealth_transfer(&statement_json, None).unwrap();
    }

    #[test]
    fn rejects_witness_without_condition_root() {
        let input_mask = RistrettoSecretKey::random(&mut rand::rng());
        let inputs_json = format!(
            r#"[{{"mask_and_value":{{"value":500,"mask":"{}"}},"witness":"KeyPath"}}]"#,
            hex::encode(input_mask.as_bytes())
        );
        let err = build_stealth_transfer_statement(&inputs_json, 0, "[]", 500).unwrap_err();
        assert!(matches!(err, OotleWasmError::Stealth(_)));
    }
}
