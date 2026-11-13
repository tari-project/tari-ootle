//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A finalized transaction updates the accounts it touches from its diff. When the diff shows an account holding a
//! vault the wallet does not know, the wallet refreshes that account from the network on the spot.

mod support;

use tari_engine_types::{
    component::{Component, ComponentBody, ComponentHeader},
    resource::Resource,
    resource_container::ResourceContainer,
    substate::{Substate, SubstateDiff, SubstateValue},
    vault::Vault,
};
use tari_ootle_common_types::{Epoch, SubstateVersion};
use tari_ootle_light_client::test_fixtures::ValidatorSetFixture;
use tari_ootle_transaction::TransactionId;
use tari_ootle_wallet_sdk::models::{KeyBranch, KeyId};
use tari_ootle_wallet_sdk_services::{account_monitor::AccountScanner, notify::Notify};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::types::{
    Amount,
    ComponentAddress,
    Metadata,
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
async fn process_result_scopes_updated_nfts_to_the_vaults_own_resource_and_emits_account_changed() {
    use std::collections::BTreeSet;

    use tari_engine_types::non_fungible::NonFungibleContainer;
    use tari_ootle_wallet_sdk::models::WalletEvent;
    use tari_template_lib::types::{NonFungibleAddress, NonFungibleId, ObjectKey};

    let nft_vault = vault_id(10);
    let nft_resource_a = ResourceAddress::new(ObjectKey::from_array([0xaa; ObjectKey::LENGTH]));
    let nft_resource_b = ResourceAddress::new(ObjectKey::from_array([0xbb; ObjectKey::LENGTH]));

    let sets = (1..=2).map(ValidatorSetFixture::new).collect::<Vec<_>>();
    let network = ProvingNetwork::new(&sets, 0);
    let test = TestWithNetwork::without_accounts(network);
    for resx in [nft_resource_a, nft_resource_b] {
        test.sdk()
            .resources_api()
            .upsert_resource(
                &resx,
                &Resource::new(
                    ResourceType::NonFungible,
                    SubstateOwnerRule::None,
                    ResourceAccessRules::new(),
                    Metadata::new(),
                    None,
                    None,
                    0,
                    false,
                ),
            )
            .unwrap();
    }
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
            nft_vault,
            SubstateVersion::ZERO,
            nft_resource_a,
            ResourceType::NonFungible,
            None,
            0,
        )
        .unwrap();

    let shared_id = NonFungibleId::from_u64(1);
    let mut diff = SubstateDiff::new();
    // Insert resource B's NFT with the same NonFungibleId first so an unscoped find() would pick it.
    diff.up(
        NonFungibleAddress::new(nft_resource_b, shared_id.clone()).into(),
        Substate::new(
            SubstateVersion::ZERO,
            NonFungibleContainer::new(
                tari_bor::to_value(&"wrong_resource_data").unwrap(),
                tari_bor::to_value(&"wrong_mutable").unwrap(),
            ),
        ),
    );
    diff.up(
        NonFungibleAddress::new(nft_resource_a, shared_id.clone()).into(),
        Substate::new(
            SubstateVersion::ZERO,
            NonFungibleContainer::new(
                tari_bor::to_value(&"correct_resource_data").unwrap(),
                tari_bor::to_value(&"correct_mutable").unwrap(),
            ),
        ),
    );
    diff.up(
        account_address().into(),
        Substate::new(VERSION, account_value(&[nft_vault])),
    );
    diff.up(
        nft_vault.into(),
        Substate::new(
            SubstateVersion::new(1),
            Vault::new(ResourceContainer::non_fungible(
                nft_resource_a,
                BTreeSet::from([shared_id.clone()]),
            )),
        ),
    );

    let notify = Notify::new(10);
    let mut rx = notify.subscribe();
    let account_scanner = AccountScanner::new(notify, test.sdk().clone());
    account_scanner
        .process_result(TransactionId::default(), &diff, None)
        .await
        .unwrap();

    let stored = test.sdk().non_fungible_api().get(nft_resource_a, shared_id).unwrap();
    assert_eq!(stored.data, tari_bor::to_value(&"correct_resource_data").unwrap());
    assert!(
        matches!(rx.try_recv(), Ok(WalletEvent::AccountChangedOnChain(_))),
        "adding an NFT to an existing NFT vault must emit AccountChangedOnChain"
    );
}
