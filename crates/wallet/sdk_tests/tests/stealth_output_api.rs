//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod support;

use tari_ootle_wallet_sdk::models::{KeyBranch, KeyId, OutputStatus, StealthOutputModel};
use tari_template_lib::types::{
    Amount,
    EncryptedData,
    constants::STEALTH_TARI_RESOURCE_ADDRESS,
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, UtxoTag},
    stealth::SpendAuthorization,
};

use crate::support::Test;

fn stealth_output(value: u64, seed: u8) -> StealthOutputModel {
    StealthOutputModel {
        owner_account: Test::test_account_address(),
        resource_address: STEALTH_TARI_RESOURCE_ADDRESS,
        commitment: PedersenCommitmentBytes::from_array([seed; PedersenCommitmentBytes::length()]),
        value,
        sender_public_nonce: RistrettoPublicKeyBytes::default(),
        view_only_key_id: KeyId::derived(KeyBranch::ViewOnlyKey, 0),
        owner_key_id: Some(KeyId::derived(KeyBranch::Account, 0)),
        encrypted_data: EncryptedData::try_from(vec![0u8; EncryptedData::min_size()]).unwrap(),
        tag_byte: UtxoTag::new(u32::from(seed)),
        memo: None,
        auth: SpendAuthorization::Key(RistrettoPublicKeyBytes::default()),
        minimum_value_promise: 0,
        status: OutputStatus::Unspent,
        is_burnt: false,
        is_frozen: false,
        is_on_chain: true,
        is_condition_spendable: true,
        lock_id: None,
    }
}

#[test]
fn burnt_outputs_are_excluded_from_the_unspent_balance() {
    let test = Test::new();
    let outputs = test.sdk().stealth_outputs_api();

    let kept = stealth_output(10, 1);
    let to_burn = stealth_output(100, 2);
    outputs.add_output(&kept).unwrap();
    outputs.add_output(&to_burn).unwrap();

    // Both outputs count while unspent and not burnt.
    assert_eq!(
        outputs
            .get_unspent_balance(&STEALTH_TARI_RESOURCE_ADDRESS)
            .unwrap()
            .balance,
        Amount::from(110u64)
    );
    let account_total: Amount = outputs
        .get_unspent_outputs_by_account(&Test::test_account_address(), false)
        .unwrap()
        .into_iter()
        .map(|o| Amount::from(o.value))
        .sum();
    assert_eq!(account_total, Amount::from(110u64));

    // A burnt output keeps status = Unspent, so it must be excluded by the burnt filter, not the status filter.
    outputs
        .update_utxo_status(&to_burn.to_utxo_address(), Some(true), None, None)
        .unwrap();

    let balance = outputs.get_unspent_balance(&STEALTH_TARI_RESOURCE_ADDRESS).unwrap();
    assert_eq!(balance.balance, Amount::from(10u64));
    assert_eq!(balance.utxo_count, 1);

    let account_outputs = outputs
        .get_unspent_outputs_by_account(&Test::test_account_address(), false)
        .unwrap();
    assert_eq!(account_outputs.len(), 1);
    assert_eq!(
        account_outputs
            .into_iter()
            .map(|o| Amount::from(o.value))
            .sum::<Amount>(),
        Amount::from(10u64)
    );
}
