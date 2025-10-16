//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use chacha20poly1305::aead::OsRng;
use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_engine_types::{
    crypto::{messages, PrivateOutput},
    resource_container::ResourceError,
    stealth,
    ToByteType,
    UtxoOutput,
};
use tari_ootle_common_types::crypto::create_key_pair_from_seed;
use tari_ootle_wallet_crypto::{
    balance_proof::generate_stealth_balance_proof_signature,
    confidential,
    stealth::{create_outputs_statement, create_transfer_statement},
    MaskAndValue,
    UnblindedOutputWitness,
    UnblindedStealthInputWitness,
    UnblindedStealthOutputWitness,
};
use tari_template_lib::types::{
    crypto::{RistrettoPublicKeyBytes, UtxoTag},
    Amount,
    EncryptedData,
};

#[test]
fn it_create_a_valid_revealed_only_proof() {
    let proof =
        confidential::create_withdraw_proof(&[], Amount::from(123), None, Amount::from(123), None, Amount::from(0))
            .unwrap();

    assert!(proof.is_revealed_only());
}

mod stealth_tests {
    use super::*;

    #[test]
    fn it_errors_for_noop_transfer() {
        let statement = create_transfer_statement(
            &[],
            Amount::zero(),
            &[],
            Amount::zero(),
            RistrettoPublicKeyBytes::zero(),
        )
        .unwrap();
        stealth::validate_transfer_balance(&statement, None).unwrap_err();
    }

    #[test]
    fn it_creates_a_valid_statement() {
        let inputs = make_input_statements(&[(1, 1000), (2, 2000), (3, 3000)]);
        let revealed_input_amount = Amount::zero();

        let output_statements = make_output_statements(&[6000]);
        let revealed_output_amount = Amount::from(0);

        let statement = create_transfer_statement(
            &inputs,
            revealed_input_amount,
            &output_statements,
            revealed_output_amount,
            RistrettoPublicKeyBytes::zero(),
        )
        .unwrap();

        stealth::validate_transfer_balance(&statement, None).unwrap();
    }

    #[test]
    fn it_creates_a_valid_statement_with_revealed() {
        let inputs = make_input_statements(&[(1, 1000), (2, 2000), (3, 3000)]);
        let revealed_input_amount = Amount::from(6000);

        let output_statements = make_output_statements(&[100, 200, 300]);
        let revealed_output_amount = Amount::from(6000 + 6000 - 100 - 200 - 300);

        let statement = create_transfer_statement(
            &inputs,
            revealed_input_amount,
            &output_statements,
            revealed_output_amount,
            RistrettoPublicKeyBytes::zero(),
        )
        .unwrap();

        stealth::validate_transfer_balance(&statement, None).unwrap();
    }

    #[test]
    fn it_creates_a_valid_statement_with_revealed_only() {
        let revealed_input_amount = Amount::from(6000);
        let revealed_output_amount = Amount::from(6000);
        let statement = create_transfer_statement(
            &[],
            revealed_input_amount,
            &[],
            revealed_output_amount,
            RistrettoPublicKeyBytes::zero(),
        )
        .unwrap();
        stealth::validate_transfer_balance(&statement, None).unwrap();

        let revealed_input_amount = Amount::from(6000);
        let revealed_output_amount = Amount::from(5999);
        let statement = create_transfer_statement(
            &[],
            revealed_input_amount,
            &[],
            revealed_output_amount,
            RistrettoPublicKeyBytes::zero(),
        )
        .unwrap();
        stealth::validate_transfer_balance(&statement, None).unwrap_err(); // Invalid, output is less than input
    }

    #[test]
    fn it_fails_to_validate_if_outputs_are_replaced() {
        let required_signer = RistrettoPublicKeyBytes::zero();
        let inputs = make_input_statements(&[(1, 1000), (2, 2000), (3, 3000)]);
        let revealed_input_amount = Amount::from(6000);

        let output_statements = make_output_statements(&[100, 200, 300]);
        let revealed_output_amount = Amount::from(6000 + 6000 - 100 - 200 - 300);

        let mut statement = create_transfer_statement(
            &inputs,
            revealed_input_amount,
            &output_statements,
            revealed_output_amount,
            required_signer,
        )
        .unwrap();

        // Make new output statements
        let output_statements = make_output_statements(&[100, 200, 300]);

        // Recreate the output statement and balance proof
        let agg_output_mask = output_statements
            .iter()
            .map(|stmt| &stmt.witness.mask)
            .fold(RistrettoSecretKey::default(), |agg, mask| agg + mask);

        let agg_input_mask = inputs
            .iter()
            .map(|stmt| &stmt.mask_and_value.mask)
            .fold(RistrettoSecretKey::default(), |agg, mask| agg + mask);

        statement.outputs_statement = create_outputs_statement(&output_statements, revealed_output_amount).unwrap();

        statement.balance_proof = generate_stealth_balance_proof_signature(
            &agg_input_mask,
            &agg_output_mask,
            &statement.inputs_statement,
            &statement.outputs_statement,
        );

        // This passes because we recreated the balance proof correctly
        stealth::validate_transfer_balance(&statement, None).unwrap();

        let metadata_hash = messages::stealth_statement_metadata64(&statement.outputs_statement);
        for (i, input) in statement.inputs_statement.inputs.iter().enumerate() {
            let original_input = &inputs.get(i).expect("input index out of range");
            // Convert the input into a UTXO to spend
            let utxo = UtxoOutput {
                output: PrivateOutput {
                    public_nonce: original_input.public_nonce.to_byte_type(),
                    // Encrypted data is not needed for ownership proof validation
                    encrypted_data: EncryptedData::empty(),
                    minimum_value_promise: 0,
                    viewable_balance: None,
                },
                owner_public_key: RistrettoPublicKey::from_secret_key(&original_input.owner_secret).to_byte_type(),
                tag: UtxoTag::new(0),
            };

            // This fails because the outputs have been malleated
            let err = stealth::validate_ownership_proof(&utxo, input, &required_signer, &metadata_hash).unwrap_err();
            assert!(matches!(err, ResourceError::InvalidSpend { .. }));
        }
    }

    fn make_input_statements(amounts: &[(u8, u64)]) -> Vec<UnblindedStealthInputWitness> {
        amounts
            .iter()
            .map(|&(seed, amount)| {
                let (mask, public_key) = create_key_pair_from_seed(seed);
                UnblindedStealthInputWitness {
                    mask_and_value: MaskAndValue::new(Amount::from(amount), mask.clone()),
                    owner_secret: mask,
                    public_nonce: public_key,
                }
            })
            .collect()
    }

    fn make_output_statements<A: Into<Amount> + Copy>(amounts: &[A]) -> Vec<UnblindedStealthOutputWitness> {
        amounts
            .iter()
            .map(|&amount| {
                let amount = amount.into();
                // If the amount is zero, we omit the output UTXO, therefore, the mask is zero
                let output_mask = if amount.is_zero() {
                    Default::default()
                } else {
                    RistrettoSecretKey::random(&mut OsRng)
                };
                // For testing purposes, we use the mask as the owner key
                let output_owner_public_key = RistrettoPublicKey::from_secret_key(&output_mask);
                let statement = UnblindedOutputWitness {
                    amount,
                    mask: output_mask,
                    resource_view_key: None,
                    // This is client/wallet on-chain data and not required for spending in tests
                    sender_public_nonce: {
                        let (_sk, pk) = create_key_pair_from_seed(0);
                        pk
                    },
                    minimum_value_promise: 0,
                    encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                };

                UnblindedStealthOutputWitness {
                    witness: statement,
                    output_owner_public_key,
                    tag: UtxoTag::new(0),
                }
            })
            .collect()
    }
}
