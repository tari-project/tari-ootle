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
use tari_ootle_common_types::{Epoch, SubstateVersion, VersionedSubstateIdRef};
use tari_ootle_light_client::test_fixtures::ValidatorSetFixture;
use tari_ootle_transaction::TransactionId;
use tari_ootle_wallet_sdk::models::{KeyBranch, KeyId};
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

fn nft_resource_address(seed: u8) -> ResourceAddress {
    format!("resource_{}", format!("{seed:02x}").repeat(32))
        .parse()
        .unwrap()
}

fn insert_nft_resource(test: &TestWithNetwork<ProvingNetwork>, res: ResourceAddress) {
    test.sdk()
        .resources_api()
        .upsert_resource(
            &res,
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

fn setup_nft_vault_wallet(nft_vault: VaultId, res_a: ResourceAddress) -> TestWithNetwork<ProvingNetwork> {
    let held = vault_id(1);
    let (_network, test) = wallet_holding(held);
    insert_nft_resource(&test, res_a);
    test.sdk()
        .accounts_api()
        .add_vault(
            account_address(),
            nft_vault,
            SubstateVersion::ZERO,
            res_a,
            ResourceType::NonFungible,
            None,
            0,
        )
        .unwrap();
    test
}

fn diff_with_two_resource_nfts(
    res_a: ResourceAddress,
    res_b: ResourceAddress,
    nft_id: &NonFungibleId,
    target_vault: VaultId,
    include_account: bool,
) -> SubstateDiff {
    let mut diff = SubstateDiff::new();
    // Insert resource B's NFT first with the colliding NonFungibleId so a resource-blind lookup would pick it.
    diff.up(
        SubstateId::NonFungible(NonFungibleAddress::new(res_b, nft_id.clone())),
        Substate::new(
            VERSION,
            NonFungibleContainer::new(tari_bor::to_value(&"from_b").unwrap(), tari_bor::Value::Null),
        ),
    );
    diff.up(
        SubstateId::NonFungible(NonFungibleAddress::new(res_a, nft_id.clone())),
        Substate::new(
            VERSION,
            NonFungibleContainer::new(tari_bor::to_value(&"from_a").unwrap(), tari_bor::Value::Null),
        ),
    );
    diff.up(
        SubstateId::Vault(target_vault),
        Substate::new(
            VERSION,
            Vault::new(ResourceContainer::non_fungible(res_a, BTreeSet::from([nft_id.clone()]))),
        ),
    );
    if include_account {
        diff.up(
            account_address().into(),
            Substate::new(VERSION, account_value(&[vault_id(1), target_vault])),
        );
    }
    diff
}

#[tokio::test]
async fn a_finalized_account_diff_matches_updated_nfts_by_resource_and_id() {
    let nft_vault = vault_id(2);
    let res_a = nft_resource_address(0xaa);
    let res_b = nft_resource_address(0xbb);
    let shared_id = NonFungibleId::from_u64(1);
    let test = setup_nft_vault_wallet(nft_vault, res_a);

    let diff = diff_with_two_resource_nfts(res_a, res_b, &shared_id, nft_vault, true);
    scanner(&test)
        .process_result(TransactionId::default(), &diff, None)
        .await
        .unwrap();

    let stored = test.sdk().non_fungible_api().get(res_a, shared_id).unwrap();
    assert_eq!(stored.data, tari_bor::to_value(&"from_a").unwrap());
}

#[tokio::test]
async fn a_finalized_standalone_vault_diff_matches_updated_nfts_by_resource_and_id() {
    let nft_vault = vault_id(2);
    let res_a = nft_resource_address(0xaa);
    let res_b = nft_resource_address(0xbb);
    let shared_id = NonFungibleId::from_u64(1);
    let test = setup_nft_vault_wallet(nft_vault, res_a);
    test.sdk()
        .substate_api()
        .save_child(
            &account_address().into(),
            VersionedSubstateIdRef::new(&SubstateId::Vault(nft_vault), SubstateVersion::ZERO),
            [],
        )
        .unwrap();

    let diff = diff_with_two_resource_nfts(res_a, res_b, &shared_id, nft_vault, false);
    scanner(&test)
        .process_result(TransactionId::default(), &diff, None)
        .await
        .unwrap();

    let stored = test.sdk().non_fungible_api().get(res_a, shared_id).unwrap();
    assert_eq!(stored.data, tari_bor::to_value(&"from_a").unwrap());
}
