//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod support;

use ootle_byte_type::ToByteType;
use tari_engine_types::{Utxo, UtxoOutput, crypto::OutputBody};
use tari_ootle_address::RistrettoOotleAddress;
use tari_ootle_wallet_sdk::{
    crypto::pay_to::PayTo,
    models::{AccountWithAddress, KeyBranch},
};
use tari_template_lib::types::{
    UtxoAddress,
    constants::STEALTH_TARI_RESOURCE_ADDRESS,
    crypto::PedersenCommitmentBytes,
};

use crate::support::{PanicNetworkInterface, Test};

fn key_index(account: &AccountWithAddress) -> u64 {
    account.account.owner_key_id.and_then(|id| id.derived_index()).unwrap()
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
fn new_accounts_take_the_lowest_unused_key_index() {
    let test = Test::without_accounts(PanicNetworkInterface);
    let accounts_api = test.sdk().accounts_api();

    let explicit = accounts_api.create_account_at_index(Some("explicit"), true, 3).unwrap();
    assert_eq!(key_index(&explicit), 3);

    let next = (0..4)
        .map(|n| {
            let account = accounts_api
                .create_next_account(Some(&format!("next-{n}")), false)
                .unwrap();
            key_index(&account)
        })
        .collect::<Vec<_>>();
    assert_eq!(next, [0, 1, 2, 4]);
}

#[test]
fn an_account_index_in_use_is_refused() {
    let test = Test::without_accounts(PanicNetworkInterface);
    let accounts_api = test.sdk().accounts_api();
    let account = accounts_api.create_next_account(None, true).unwrap();

    let err = accounts_api
        .create_account_at_index(Some("again"), false, key_index(&account))
        .unwrap_err();
    assert!(err.is_account_exists_error(), "{err}");
}

#[test]
fn a_public_key_created_at_an_explicit_index_records_the_index() {
    let test = Test::without_accounts(PanicNetworkInterface);
    let km = test.sdk().key_manager_api();

    let key = km.create_public_key_at_index(KeyBranch::Transaction, 3).unwrap();
    assert_eq!(key.key_id.derived_index(), Some(3));
    let listed = km
        .get_all_derived_keys(KeyBranch::Transaction)
        .unwrap()
        .into_iter()
        .map(|key| key.key_id().derived_index())
        .collect::<Vec<_>>();
    assert_eq!(listed, [Some(3)]);

    let next = km.next_public_key(KeyBranch::Transaction).unwrap();
    assert_eq!(next.key_id.derived_index(), Some(4));
}

#[test]
fn an_account_at_an_explicit_index_receives_stealth_outputs() {
    let sender = Test::new();
    let receiver = Test::without_accounts(PanicNetworkInterface);
    let account = receiver
        .sdk()
        .accounts_api()
        .create_account_at_index(Some("explicit"), true, 3)
        .unwrap();
    let address = receiver.sdk().key_manager_api().derive_account_address(3).unwrap();

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
