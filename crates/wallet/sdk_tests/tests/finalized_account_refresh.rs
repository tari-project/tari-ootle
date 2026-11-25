//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A finalized transaction updates the accounts it touches from its diff. When the diff shows an account holding a
//! vault the wallet does not know, the wallet refreshes that account from the network on the spot.

mod support;

use std::collections::BTreeSet;

use tari_engine_types::{
    component::{Component, ComponentBody, ComponentHeader},
    non_fungible::NonFungibleContainer,
    resource::Resource,
    resource_container::ResourceContainer,
    substate::{Substate, SubstateDiff, SubstateId, SubstateValue},
    vault::Vault,
};
use tari_ootle_common_types::{Epoch, SubstateVersion};
use tari_ootle_light_client::test_fixtures::ValidatorSetFixture;
use tari_ootle_transaction::TransactionId;
use tari_ootle_wallet_sdk::models::{BalanceChangeSource, KeyBranch, KeyId, NonFungibleToken, WalletEvent};
use tari_ootle_wallet_sdk_services::{account_monitor::AccountScanner, notify::Notify};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::types::{
    Amount,
    ComponentAddress,
    Metadata,
    NonFungibleAddress,
    NonFungibleId,
    ResourceAddress,
    ResourceType,
    SubstateOwnerRule,
    VaultId,
    access_rules::{ComponentAccessRules, ResourceAccessRules},
};
use tokio::sync::broadcast;

use crate::support::{ProvingNetwork, Test, TestSdkSpec, TestWithNetwork};

const VERSION: SubstateVersion = SubstateVersion::new(3);

fn account_address() -> ComponentAddress {
    Test::test_account_address()
}

fn resource_address() -> ResourceAddress {
    "resource_7cbfe29101c24924b1b6ccefbfff98986d648622272ae24f7585dab5ffffffff"
        .parse()
        .unwrap()
}

fn vault_id(seed: u8) -> VaultId {
    VaultId::from_hex(&format!("{seed:02x}").repeat(32)).unwrap()
}

fn account_value(vaults: &[VaultId]) -> SubstateValue {
    Component {
        header: ComponentHeader {
            template_address: ACCOUNT_TEMPLATE_ADDRESS,
            owner_rule: SubstateOwnerRule::None,
            access_rules: ComponentAccessRules::new(),
            entity_id: Default::default(),
            upgrade_rule: Default::default(),
            rules_updater: None,
        },
        body: ComponentBody::from_cbor_value(tari_bor::to_value(&vaults.to_vec()).unwrap()),
    }
    .into()
}

fn vault_value(amount: u64) -> SubstateValue {
    Vault::new(ResourceContainer::public_fungible(resource_address(), amount)).into()
}

/// A wallet holding the test account with `held` as its only vault, and the network serving it.
fn wallet_holding(held: VaultId) -> (ProvingNetwork, TestWithNetwork<ProvingNetwork>) {
    let sets = (1..=2).map(ValidatorSetFixture::new).collect::<Vec<_>>();
    let network = ProvingNetwork::new(&sets, 0);
    let test = TestWithNetwork::without_accounts(network.clone());
    test.sdk()
        .resources_api()
        .upsert_resource(
            &resource_address(),
            &Resource::new(
                ResourceType::Fungible,
                SubstateOwnerRule::None,
                ResourceAccessRules::new(),
                Metadata::new(),
                None,
                None,
                6,
                false,
            ),
        )
        .unwrap();
    let accounts_api = test.sdk().accounts_api();
    accounts_api
        .add_account(
            Some("test"),
            &account_address(),
            KeyId::derived(KeyBranch::ViewOnlyKey, 0),
            KeyId::derived(KeyBranch::Account, 0),
            Epoch::zero(),
            true,
            true,
        )
        .unwrap();
    accounts_api
        .add_vault(
            account_address(),
            held,
            SubstateVersion::ZERO,
            resource_address(),
            ResourceType::Fungible,
            None,
            6,
        )
        .unwrap();
    (network, test)
}

fn scanner(test: &TestWithNetwork<ProvingNetwork>) -> AccountScanner<TestSdkSpec<ProvingNetwork>> {
    AccountScanner::new(Notify::new(1), test.sdk().clone())
}

fn diff_with_account(vaults: &[VaultId]) -> SubstateDiff {
    let mut diff = SubstateDiff::new();
    diff.up(account_address().into(), Substate::new(VERSION, account_value(vaults)));
    diff
}

#[tokio::test]
async fn a_finalized_account_holding_an_unknown_vault_is_refreshed() {
    let held = vault_id(1);
    let unknown = vault_id(2);
    let (network, test) = wallet_holding(held);
    network.serve_substate(account_address().into(), VERSION, account_value(&[held, unknown]), None);
    network.serve_substate(held.into(), VERSION, vault_value(100), None);
    network.serve_substate(unknown.into(), VERSION, vault_value(50), None);

    scanner(&test)
        .process_result(TransactionId::default(), &diff_with_account(&[held, unknown]), None)
        .await
        .unwrap();

    let vault = test.sdk().accounts_api().get_vault(&unknown).unwrap();
    assert_eq!(vault.account_address, account_address());
    assert_eq!(vault.revealed_balance, Amount::from(50u64));
}

#[tokio::test]
async fn a_finalized_account_holding_only_known_vaults_is_not_read_from_the_network() {
    let held = vault_id(1);
    // The network serves no account, so a refresh panics on reading it.
    let (_network, test) = wallet_holding(held);

    scanner(&test)
        .process_result(TransactionId::default(), &diff_with_account(&[held]), None)
        .await
        .unwrap();
}

fn nft_id() -> NonFungibleId {
    NonFungibleId::from_u64(7)
}

fn nft_address() -> NonFungibleAddress {
    NonFungibleAddress::new(resource_address(), nft_id())
}

fn nft_substate(mutable_data: u64) -> Substate {
    Substate::new(
        SubstateVersion::new(1),
        NonFungibleContainer::new(
            tari_bor::Value::Text("immutable".to_string()),
            tari_bor::Value::Integer(mutable_data.into()),
        ),
    )
}

fn nft_vault_substate() -> Substate {
    Substate::new(
        SubstateVersion::new(1),
        Vault::new(ResourceContainer::non_fungible(
            resource_address(),
            BTreeSet::from([nft_id()]),
        )),
    )
}

/// A wallet whose `held` vault holds the test NFT with mutable data `1`, and a scanner whose events the receiver gets.
fn wallet_holding_nft(
    held: VaultId,
) -> (
    TestWithNetwork<ProvingNetwork>,
    AccountScanner<TestSdkSpec<ProvingNetwork>>,
    broadcast::Receiver<WalletEvent>,
) {
    let (_network, test) = wallet_holding(held);
    test.sdk()
        .non_fungible_api()
        .save_nft(&NonFungibleToken {
            vault_id: held,
            nft_id: nft_id(),
            resource_address: resource_address(),
            data: tari_bor::Value::Text("immutable".to_string()),
            mutable_data: tari_bor::Value::Integer(1.into()),
            is_burnt: false,
        })
        .unwrap();
    let notify = Notify::new(8);
    let events = notify.subscribe();
    let scanner = AccountScanner::new(notify, test.sdk().clone());
    (test, scanner, events)
}

fn assert_nft_updated(
    test: &TestWithNetwork<ProvingNetwork>,
    events: &mut broadcast::Receiver<WalletEvent>,
    mutable_data: u64,
) {
    let updated = test.sdk().non_fungible_api().get(resource_address(), nft_id()).unwrap();
    assert_eq!(updated.mutable_data, tari_bor::Value::Integer(mutable_data.into()));
    assert!(matches!(
        events.try_recv().unwrap(),
        WalletEvent::AccountChangedOnChain(_)
    ));
}

#[tokio::test]
async fn a_standalone_nft_update_updates_the_held_nft_and_notifies_account_changed() {
    let (test, scanner, mut events) = wallet_holding_nft(vault_id(1));
    // `UpdateNonFungibleData` ups the NFT substate without its vault.
    let mut diff = SubstateDiff::new();
    diff.up(SubstateId::NonFungible(nft_address()), nft_substate(2));

    scanner
        .process_result(TransactionId::default(), &diff, None)
        .await
        .unwrap();

    assert_nft_updated(&test, &mut events, 2);
}

#[tokio::test]
async fn an_nft_update_in_an_unchanged_account_vault_updates_the_held_nft_and_notifies_account_changed() {
    let held = vault_id(1);
    let (test, scanner, mut events) = wallet_holding_nft(held);
    // The vault's balance and NFT set match the diff, so only the NFT data changes.
    test.sdk()
        .accounts_api()
        .update_vault_balance_and_record_change(
            held,
            SubstateVersion::ZERO,
            Amount::from(1u64),
            Amount::zero(),
            BalanceChangeSource::Scan,
        )
        .unwrap();
    let mut diff = diff_with_account(&[held]);
    diff.up(SubstateId::Vault(held), nft_vault_substate());
    diff.up(SubstateId::NonFungible(nft_address()), nft_substate(3));

    scanner
        .process_result(TransactionId::default(), &diff, None)
        .await
        .unwrap();

    assert_nft_updated(&test, &mut events, 3);
}

#[tokio::test]
async fn an_nft_update_in_an_unrefreshed_vault_updates_the_held_nft_and_notifies_account_changed() {
    let held = vault_id(1);
    let (test, scanner, mut events) = wallet_holding_nft(held);
    // The diff ups the vault without its account, and the wallet holds no substate record for the vault, so neither
    // vault loop refreshes it.
    let mut diff = SubstateDiff::new();
    diff.up(SubstateId::Vault(held), nft_vault_substate());
    diff.up(SubstateId::NonFungible(nft_address()), nft_substate(4));

    scanner
        .process_result(TransactionId::default(), &diff, None)
        .await
        .unwrap();

    assert_nft_updated(&test, &mut events, 4);
}
