//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use chacha20poly1305::aead::OsRng;
use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_engine_types::stealth;
use tari_ootle_common_types::crypto::create_key_pair_from_seed;
use tari_ootle_wallet_crypto::{
    confidential,
    stealth::create_transfer_statement,
    MaskAndValue,
    UnblindedOutputStatement,
    UnblindedStealthInputStatement,
    UnblindedStealthOutputStatement,
};
use tari_template_lib::{models::EncryptedData, types::Amount};
use tari_template_test_tooling::support::stealth::test_sender_public_nonce;

#[test]
fn it_create_a_valid_revealed_only_proof() {
    let proof =
        confidential::create_withdraw_proof(&[], Amount::from(123), None, Amount::from(123), None, Amount::from(0))
            .unwrap();

    assert!(proof.is_revealed_only());
}

mod stealth_tests {
    use tari_template_lib::types::crypto::UtxoTagByte;

    use super::*;

    #[test]
    fn it_errors_for_noop_transfer() {
        let statement = create_transfer_statement(&[], Amount::zero(), &[], Amount::zero()).unwrap();
        stealth::validate_transfer(&statement, None).unwrap_err();
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
        )
        .unwrap();

        stealth::validate_transfer(&statement, None).unwrap();
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
        )
        .unwrap();

        stealth::validate_transfer(&statement, None).unwrap();
    }

    #[test]
    fn it_creates_a_valid_statement_with_revealed_only() {
        let revealed_input_amount = Amount::from(6000);
        let revealed_output_amount = Amount::from(6000);
        let statement = create_transfer_statement(&[], revealed_input_amount, &[], revealed_output_amount).unwrap();
        stealth::validate_transfer(&statement, None).unwrap();

        let revealed_input_amount = Amount::from(6000);
        let revealed_output_amount = Amount::from(5999);
        let statement = create_transfer_statement(&[], revealed_input_amount, &[], revealed_output_amount).unwrap();
        stealth::validate_transfer(&statement, None).unwrap_err(); // Invalid, output is less than input
    }

    fn make_input_statements(amounts: &[(u8, u64)]) -> Vec<UnblindedStealthInputStatement> {
        amounts
            .iter()
            .map(|&(seed, amount)| {
                let (mask, public_key) = create_key_pair_from_seed(seed);
                UnblindedStealthInputStatement {
                    mask_and_value: MaskAndValue::new(Amount::from(amount), mask.clone()),
                    owner_secret: mask,
                    public_nonce: public_key,
                }
            })
            .collect()
    }

    fn make_output_statements<A: Into<Amount> + Copy>(amounts: &[A]) -> Vec<UnblindedStealthOutputStatement> {
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
                let statement = UnblindedOutputStatement {
                    amount,
                    mask: output_mask,
                    resource_view_key: None,
                    // This is client/wallet on-chain data and not required for spending in tests
                    sender_public_nonce: test_sender_public_nonce(),
                    minimum_value_promise: 0,
                    encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                };

                UnblindedStealthOutputStatement {
                    statement,
                    output_owner_public_key,
                    tag: UtxoTagByte::new(0),
                }
            })
            .collect()
    }
}
