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
/// separately, this wraps `create_transfer_statement` directly -- the single primitive that
/// produces the *entire*, internally-consistent statement in one call, including a covenant
/// balance-integrity proof for any script-path-spent input whose revealed leaf gates on
/// `Covenant::BalancePreserved` (or a `TemplateFunction` calling `SpendContext::covenant_balanced`).
/// A revealed leaf that gates on something else -- a `HashLock`, an `AfterEpoch`/`BeforeEpoch`
/// timelock, an `AccessRule` -- never reads `covenant_claims`, so this call is unneeded for those
/// spends; the separate-calls path above still produces a valid statement for them, just with no
/// (unused) covenant claim attached.
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
    use tari_engine_types::crypto::validate_covenant_balance_proof;
    use tari_template_lib_types::{
        EncryptedData,
        Hash32,
        crypto::UtxoTag,
        stealth::{Covenant, SpendAuthorization, SpendCondition, StealthTransferStatement},
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
    fn explicit_key_path_witness_without_condition_root_is_accepted() {
        let input_mask = RistrettoSecretKey::random(&mut rand::rng());
        let output_mask = RistrettoSecretKey::random(&mut rand::rng());

        let inputs_json = format!(
            r#"[{{"mask_and_value":{{"value":500,"mask":"{}"}},"witness":"KeyPath"}}]"#,
            hex::encode(input_mask.as_bytes())
        );
        let outputs_json = format!("[{}]", output_witness_json(500, &output_mask));

        let statement_json = build_stealth_transfer_statement(&inputs_json, 0, &outputs_json, 0).unwrap();
        let statement: StealthTransferStatement = serde_json::from_str(&statement_json).unwrap();
        assert!(statement.covenant_claims.is_empty());

        crate::stealth::validate::validate_stealth_transfer(&statement_json, None).unwrap();
    }

    /// A script-path input whose revealed leaf gates on `Covenant::BalancePreserved` is exactly the case
    /// `covenant_claims` exists for -- the engine's `SpendScriptExecution::covenant_balanced` reconstructs the same
    /// partition from the same statement and calls the same `validate_covenant_balance_proof` primitive this test
    /// calls directly, so verifying against it here (rather than only checking the claim's shape) exercises what the
    /// engine actually does with the claim.
    #[test]
    fn script_path_input_produces_a_verifiable_covenant_claim() {
        let condition = SpendCondition::covenant(Covenant::BalancePreserved(0));
        let conditions = vec![condition.clone()];
        let conditions_json = serde_json::to_string(&conditions).unwrap();
        let claim_leaf_json = serde_json::to_string(&condition).unwrap();

        let witness_result_json = build_script_path_witness(&conditions_json, &claim_leaf_json, &[]).unwrap();
        let witness_result: serde_json::Value = serde_json::from_str(&witness_result_json).unwrap();
        let condition_root: Hash32 = serde_json::from_value(witness_result["condition_root"].clone()).unwrap();

        let input_mask = RistrettoSecretKey::random(&mut rand::rng());
        let output_mask = RistrettoSecretKey::random(&mut rand::rng());
        let inputs_json = format!(
            r#"[{{"mask_and_value":{{"value":500,"mask":"{}"}},"witness":{},"condition_root":{}}}]"#,
            hex::encode(input_mask.as_bytes()),
            witness_result["witness"],
            witness_result["condition_root"]
        );

        // Re-locks the full 500 under exactly `condition_root`, with no key-path escape, so the partition's value is
        // fully conserved -- the case `BalancePreserved(0)` (no cleartext outflow) admits.
        let owner_pk = RistrettoPublicKey::from_secret_key(&output_mask);
        let outputs_json = format!(
            "[{}]",
            serde_json::to_string(&StealthOutputWitnessJson {
                witness: crate::stealth::types::OutputWitnessJson {
                    amount: 500,
                    mask: hex::encode(output_mask.as_bytes()),
                    sender_public_nonce: hex::encode(owner_pk.as_bytes()),
                    minimum_value_promise: 0,
                    encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                    resource_view_key: None,
                },
                auth: SpendAuthorization::Script(condition_root),
                tag: UtxoTag::new(0),
            })
            .unwrap()
        );

        let statement_json = build_stealth_transfer_statement(&inputs_json, 0, &outputs_json, 0).unwrap();
        let statement: StealthTransferStatement = serde_json::from_str(&statement_json).unwrap();

        assert_eq!(statement.covenant_claims.len(), 1);
        let claim = &statement.covenant_claims[0];
        assert_eq!(claim.partition_input_index, 0);
        assert_eq!(claim.revealed_amount, tari_template_lib_types::Amount::zero());

        // Reconstruct the partition exactly as `SpendScriptExecution::covenant_balanced` does: every input/output
        // commitment sharing `condition_root`, with `KeyAndScript` outputs excluded (they don't stay in the vault).
        let input_commitments = vec![statement.inputs_statement.inputs[0].commitment];
        let output_commitments: Vec<_> = statement
            .outputs_statement
            .outputs
            .iter()
            .filter(|o| matches!(&o.auth, SpendAuthorization::Script(root) if *root == condition_root))
            .map(|o| o.output.commitment)
            .collect();
        assert_eq!(output_commitments.len(), 1);
        assert!(
            validate_covenant_balance_proof(
                &condition_root,
                claim.revealed_amount,
                &input_commitments,
                &output_commitments,
                &claim.signature,
            ),
            "the claim must verify against the same primitive the engine's covenant_balanced() calls"
        );

        // A claim asserting the wrong revealed amount for this partition must not verify -- otherwise the assertions
        // above would equally hold for a claim whose signature proves nothing about this partition's balance.
        assert!(!validate_covenant_balance_proof(
            &condition_root,
            tari_template_lib_types::Amount::from_u64(1),
            &input_commitments,
            &output_commitments,
            &claim.signature,
        ));

        crate::stealth::validate::validate_stealth_transfer(&statement_json, None).unwrap();
    }

    #[test]
    fn rejects_script_path_witness_without_condition_root() {
        let conditions = vec![SpendCondition::covenant(Covenant::BalancePreserved(0))];
        let conditions_json = serde_json::to_string(&conditions).unwrap();
        let claim_leaf_json = serde_json::to_string(&conditions[0]).unwrap();
        let witness_result_json = build_script_path_witness(&conditions_json, &claim_leaf_json, &[]).unwrap();
        let witness_result: serde_json::Value = serde_json::from_str(&witness_result_json).unwrap();

        let input_mask = RistrettoSecretKey::random(&mut rand::rng());
        let inputs_json = format!(
            r#"[{{"mask_and_value":{{"value":500,"mask":"{}"}},"witness":{}}}]"#,
            hex::encode(input_mask.as_bytes()),
            witness_result["witness"],
        );
        let err = build_stealth_transfer_statement(&inputs_json, 0, "[]", 500).unwrap_err();
        assert!(matches!(err, OotleWasmError::Stealth(_)));
    }

    #[test]
    fn rejects_key_path_witness_with_condition_root() {
        let input_mask = RistrettoSecretKey::random(&mut rand::rng());
        let inputs_json = format!(
            r#"[{{"mask_and_value":{{"value":500,"mask":"{}"}},"witness":"KeyPath","condition_root":"{}"}}]"#,
            hex::encode(input_mask.as_bytes()),
            hex::encode(Hash32::zero().as_slice())
        );
        let err = build_stealth_transfer_statement(&inputs_json, 0, "[]", 500).unwrap_err();
        assert!(matches!(err, OotleWasmError::Stealth(_)));
    }
}
