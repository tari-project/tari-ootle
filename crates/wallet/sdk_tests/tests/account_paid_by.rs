//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod support;

use ootle_byte_type::ToByteType;
use tari_crypto::{keys::PublicKey as _, ristretto::RistrettoPublicKey};
use tari_ootle_address::{DestinationTag, Network, OotleAddress};
use tari_ootle_wallet_sdk::models::AccountWithAddress;
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;

use crate::support::{PanicNetworkInterface, Test, random_key};

fn wallet_with_account() -> (Test, AccountWithAddress) {
    let test = Test::without_accounts(PanicNetworkInterface);
    let address = test.sdk().key_manager_api().next_account_address().unwrap();
    let account = test.sdk().accounts_api().create_account(None, true, address).unwrap();
    (test, account)
}

fn other_key() -> RistrettoPublicKeyBytes {
    RistrettoPublicKey::from_secret_key(&random_key()).to_byte_type()
}

#[test]
fn the_accounts_own_address_finds_it() {
    let (test, account) = wallet_with_account();
    let found = test
        .sdk()
        .accounts_api()
        .find_account_paid_by(account.address())
        .unwrap();
    assert_eq!(
        found.map(|a| *a.component_address()),
        Some(*account.component_address())
    );
}

#[test]
fn a_destination_tag_does_not_change_the_account() {
    let (test, account) = wallet_with_account();
    let tagged = account
        .address()
        .clone()
        .with_destination_tag(DestinationTag::new_checked(*b"invoice-42").unwrap());
    let found = test.sdk().accounts_api().find_account_paid_by(&tagged).unwrap();
    assert_eq!(
        found.map(|a| *a.component_address()),
        Some(*account.component_address())
    );
}

#[test]
fn the_account_key_under_another_view_key_is_not_the_account() {
    let (test, account) = wallet_with_account();
    let address = OotleAddress::new(
        account.address().network(),
        other_key(),
        *account.address().account_public_key(),
    );
    assert!(
        test.sdk()
            .accounts_api()
            .find_account_paid_by(&address)
            .unwrap()
            .is_none()
    );
}

#[test]
fn the_accounts_keys_on_another_network_are_not_the_account() {
    let (test, account) = wallet_with_account();
    let own = account.address();
    assert_ne!(own.network(), Network::MainNet);
    let address = OotleAddress::new(Network::MainNet, *own.view_only_key(), *own.account_public_key());
    assert!(
        test.sdk()
            .accounts_api()
            .find_account_paid_by(&address)
            .unwrap()
            .is_none()
    );
}

#[test]
fn an_address_of_no_wallet_account_finds_nothing() {
    let (test, account) = wallet_with_account();
    let address = OotleAddress::new(account.address().network(), other_key(), other_key());
    assert!(
        test.sdk()
            .accounts_api()
            .find_account_paid_by(&address)
            .unwrap()
            .is_none()
    );
}
