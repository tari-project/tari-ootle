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
use tari_ootle_wallet_sdk::models::{KeyBranch, KeyId, NonFungibleToken, WalletEvent};
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

#[tokio::test]
async fn process_result_updates_existing_nft_mutable_data_and_notifies_account_changed() {
    let held = vault_id(1);
    let (_network, test) = wallet_holding(held);
    let nft_id = NonFungibleId::from_u64(7);
    let nft_address = NonFungibleAddress::new(resource_address(), nft_id.clone());

    test.sdk()
        .non_fungible_api()
        .save_nft(&NonFungibleToken {
            vault_id: held,
            nft_id: nft_id.clone(),
            resource_address: resource_address(),
            data: tari_bor::Value::Text("immutable".to_string()),
            mutable_data: tari_bor::Value::Integer(1.into()),
            is_burnt: false,
        })
        .unwrap();

    // Case 1: Standalone NFT substate update without a vault substate in the diff (`UpdateNonFungibleData`).
    let notify = Notify::new(8);
    let mut sub = notify.subscribe();
    let scanner = AccountScanner::new(notify, test.sdk().clone());
    let mut standalone_diff = SubstateDiff::new();
    standalone_diff.up(
        SubstateId::NonFungible(nft_address.clone()),
        Substate::new(
            SubstateVersion::new(1),
            NonFungibleContainer::new(
                tari_bor::Value::Text("immutable".to_string()),
                tari_bor::Value::Integer(2.into()),
            ),
        ),
    );
    scanner
        .process_result(TransactionId::default(), &standalone_diff, None)
        .await
        .unwrap();

    let updated = test
        .sdk()
        .non_fungible_api()
        .nft_get_by_id(&resource_address(), &nft_id)
        .unwrap();
    assert_eq!(updated.mutable_data, tari_bor::Value::Integer(2.into()));
    assert!(matches!(sub.try_recv().unwrap(), WalletEvent::AccountChangedOnChain(_)));

    // Case 2: Both the vault (with unchanged NFT set/balance) and the NFT substate appear in the diff.
    let mut vault_and_nft_diff = SubstateDiff::new();
    vault_and_nft_diff.up(
        SubstateId::Vault(held),
        Substate::new(
            SubstateVersion::new(1),
            Vault::new(ResourceContainer::non_fungible(
                resource_address(),
                BTreeSet::from([nft_id.clone()]),
            )),
        ),
    );
    vault_and_nft_diff.up(
        SubstateId::NonFungible(nft_address),
        Substate::new(
            SubstateVersion::new(2),
            NonFungibleContainer::new(
                tari_bor::Value::Text("immutable".to_string()),
                tari_bor::Value::Integer(3.into()),
            ),
        ),
    );
    scanner
        .process_result(TransactionId::default(), &vault_and_nft_diff, None)
        .await
        .unwrap();

    let updated = test
        .sdk()
        .non_fungible_api()
        .nft_get_by_id(&resource_address(), &nft_id)
        .unwrap();
    assert_eq!(updated.mutable_data, tari_bor::Value::Integer(3.into()));
    assert!(matches!(sub.try_recv().unwrap(), WalletEvent::AccountChangedOnChain(_)));
}
