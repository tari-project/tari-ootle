//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::iter;

use chacha20poly1305::aead::OsRng;
use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_engine_types::stealth;
use tari_ootle_common_types::crypto::create_key_pair_from_seed;
use tari_ootle_wallet_crypto::{
    MaskAndValue,
    OutputWitness,
    StealthInputWitness,
    StealthOutputWitness,
    confidential,
    stealth::create_transfer_statement,
};
use tari_template_lib_types::{Amount, EncryptedData, crypto::UtxoTag};

#[test]
fn it_create_a_valid_revealed_only_proof() {
    let proof = confidential::create_withdraw_proof(
        &[],
        Amount::from(123u64),
        None,
        Amount::from(123u64),
        None,
        Amount::from(0u64),
    )
    .unwrap();

    assert!(proof.is_revealed_only());
}

mod stealth_tests {
    use ootle_byte_type::ToByteType;
    use tari_template_lib_types::stealth::SpendCondition;

    use super::*;

    #[test]
    fn it_errors_for_noop_transfer() {
        let statement =
            create_transfer_statement(iter::empty(), Amount::zero(), iter::empty(), Amount::zero()).unwrap();
        stealth::validate_transfer(&statement, None).unwrap_err();
    }

    #[test]
    fn it_creates_a_valid_statement() {
        let inputs = make_input_statements(&[(1, 1000), (2, 2000), (3, 3000)]);
        let revealed_input_amount = Amount::zero();

        let output_statements = make_output_statements(&[6000]);
        let revealed_output_amount = Amount::from(0u64);

        let statement = create_transfer_statement(
            inputs,
            revealed_input_amount,
            output_statements.iter(),
            revealed_output_amount,
        )
        .unwrap();

        stealth::validate_transfer(&statement, None).unwrap();
    }

    #[test]
    fn it_creates_a_valid_statement_with_revealed() {
        let inputs = make_input_statements(&[(1, 1000), (2, 2000), (3, 3000)]);
        let revealed_input_amount = Amount::from(6000u64);

        let output_statements = make_output_statements(&[100, 200, 300]);
        let revealed_output_amount = Amount::from_u64(6000 + 6000 - 100 - 200 - 300);

        let statement = create_transfer_statement(
            inputs,
            revealed_input_amount,
            output_statements.iter(),
            revealed_output_amount,
        )
        .unwrap();

        stealth::validate_transfer(&statement, None).unwrap();
    }

    #[test]
    fn it_creates_a_valid_statement_with_revealed_only() {
        let revealed_input_amount = Amount::from(6000u64);
        let revealed_output_amount = Amount::from(6000u64);
        let statement = create_transfer_statement(
            iter::empty(),
            revealed_input_amount,
            iter::empty(),
            revealed_output_amount,
        )
        .unwrap();
        stealth::validate_transfer(&statement, None).unwrap();

        let revealed_input_amount = Amount::from(6000u64);
        let revealed_output_amount = Amount::from(5999u64);
        let statement = create_transfer_statement(
            iter::empty(),
            revealed_input_amount,
            iter::empty(),
            revealed_output_amount,
        )
        .unwrap();
        stealth::validate_transfer(&statement, None).unwrap_err(); // Invalid, output is less than input
    }

    fn make_input_statements(amounts: &[(u8, u64)]) -> Vec<StealthInputWitness> {
        amounts
            .iter()
            .map(|&(seed, amount)| {
                let (mask, _) = create_key_pair_from_seed(seed);
                StealthInputWitness {
                    mask_and_value: MaskAndValue::new(amount, mask.clone()),
                }
            })
            .collect()
    }

    fn make_output_statements(amounts: &[u64]) -> Vec<StealthOutputWitness> {
        amounts
            .iter()
            .filter(|amount| **amount > 0)
            .map(|&amount| {
                let output_mask = RistrettoSecretKey::random(&mut OsRng);
                // For testing purposes, we use the mask as the owner key
                let output_owner_public_key = RistrettoPublicKey::from_secret_key(&output_mask);
                let statement = OutputWitness {
                    amount,
                    mask: output_mask,
                    resource_view_key: None,
                    // This is client/wallet on-chain data and not required for spending in tests
                    sender_public_nonce: {
                        let (_sk, pk) = create_key_pair_from_seed(0);
                        pk
                    },
                    minimum_value_promise: 0,
                    encrypted_data: EncryptedData::empty(),
                };

                StealthOutputWitness {
                    witness: statement,
                    spend_condition: SpendCondition::Signed(output_owner_public_key.to_byte_type()),
                    tag: UtxoTag::new(0),
                }
            })
            .collect()
    }
}
