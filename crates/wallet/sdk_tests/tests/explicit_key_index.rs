//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod support;

use ootle_byte_type::ToByteType;
use tari_engine_types::{Utxo, UtxoOutput, crypto::OutputBody};
use tari_ootle_address::RistrettoOotleAddress;
use tari_ootle_wallet_sdk::{crypto::pay_to::PayTo, models::KeyBranch};
use tari_template_lib::types::{
    UtxoAddress,
    constants::STEALTH_TARI_RESOURCE_ADDRESS,
    crypto::PedersenCommitmentBytes,
};

use crate::support::{PanicNetworkInterface, Test};

fn derived_indices(test: &Test, branch: KeyBranch) -> Vec<u64> {
    test.sdk()
        .key_manager_api()
        .get_all_derived_keys(branch)
        .unwrap()
        .into_iter()
        .map(|key| key.key_id().derived_index().unwrap())
        .collect()
}

/// An output `sender` pays to `destination`, encrypted and tagged for its view key.
fn stealth_output_to(sender: &Test, destination: &RistrettoOotleAddress, value: u64) -> (UtxoAddress, Utxo) {
    let output = sender
        .sdk()
        .stealth_outputs_api()
        .create_output_witness(
            destination,
            value,
            &STEALTH_TARI_RESOURCE_ADDRESS,
            None,
            None,
            PayTo::StealthPublicKey,
        )
        .unwrap();
    let commitment: PedersenCommitmentBytes = output.witness.to_commitment().to_byte_type();
    let utxo = Utxo::new(UtxoOutput {
        output: OutputBody {
            public_nonce: output.witness.sender_public_nonce.to_byte_type(),
            encrypted_data: output.witness.encrypted_data.clone(),
            minimum_value_promise: 0,
            viewable_balance: None,
        },
        auth: output.auth,
        tag: output.tag,
    });
    (UtxoAddress::new(STEALTH_TARI_RESOURCE_ADDRESS, commitment.into()), utxo)
}

#[test]
fn an_account_address_created_at_an_explicit_index_records_the_index() {
    let test = Test::without_accounts(PanicNetworkInterface);
    let km = test.sdk().key_manager_api();

    let address = km.create_account_address_at_index(5).unwrap();
    assert_eq!(address.owner_key_id.derived_index(), Some(5));
    assert_eq!(derived_indices(&test, KeyBranch::Account), [5]);
    assert_eq!(derived_indices(&test, KeyBranch::ViewOnlyKey), [5]);

    let next = km.next_account_address().unwrap();
    assert_eq!(next.owner_key_id.derived_index(), Some(6));
}

#[test]
fn a_public_key_created_at_an_explicit_index_records_the_index() {
    let test = Test::without_accounts(PanicNetworkInterface);
    let km = test.sdk().key_manager_api();

    let key = km.create_public_key_at_index(KeyBranch::Transaction, 3).unwrap();
    assert_eq!(key.key_id.derived_index(), Some(3));
    assert_eq!(derived_indices(&test, KeyBranch::Transaction), [3]);
    assert!(derived_indices(&test, KeyBranch::ViewOnlyKey).is_empty());

    let next = km.next_public_key(KeyBranch::Transaction).unwrap();
    assert_eq!(next.key_id.derived_index(), Some(4));
}

#[test]
fn an_account_at_an_unrecorded_index_receives_stealth_outputs() {
    let sender = Test::new();
    let receiver = Test::without_accounts(PanicNetworkInterface);
    // `derive_account_address` records nothing, so the key manager has no view key row for index 3
    let address = receiver.sdk().key_manager_api().derive_account_address(3).unwrap();
    let account = receiver
        .sdk()
        .accounts_api()
        .create_account(Some("explicit"), true, address.clone())
        .unwrap();
    assert!(derived_indices(&receiver, KeyBranch::ViewOnlyKey).is_empty());

    let (utxo_address, utxo) = stealth_output_to(&sender, &address.address, 1234);
    receiver
        .sdk()
        .stealth_outputs_api()
        .verify_and_update_outputs([(utxo_address, &utxo)])
        .unwrap();

    let outputs = receiver
        .sdk()
        .stealth_outputs_api()
        .utxos_get_many(&STEALTH_TARI_RESOURCE_ADDRESS, Some(account.component_address()), None)
        .unwrap();
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].value, 1234);
}
