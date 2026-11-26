//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! An account that holds a live stealth output of a resource is scanned for that resource, so the wallet sees later
//! changes to the output that another wallet makes, such as the issuer freezing it. This holds however the wallet came
//! to hold the output: found in a finalized transaction's diff, or recorded up front for a transfer statement paying
//! the account.

mod support;

use tari_engine_types::resource::Resource;
use tari_ootle_common_types::SubstateVersion;
use tari_ootle_light_client::test_fixtures::ValidatorSetFixture;
use tari_ootle_wallet_sdk::{
    IndexerTrust,
    models::{AccountWithAddress, OutputStatus, StealthOutputModel},
};
use tari_ootle_wallet_sdk_services::{account_monitor::AccountScanner, notify::Notify};
use tari_template_lib::types::{
    ComponentAddress,
    Metadata,
    ResourceAddress,
    ResourceType,
    SubstateOwnerRule,
    access_rules::ResourceAccessRules,
};

use crate::support::{ProvingNetwork, TestWithNetwork, random_key};

type Wallet = TestWithNetwork<ProvingNetwork>;

fn cached_resource_address() -> ResourceAddress {
    "resource_38defd897ba069aff5038b8045fdbea5bbd94ed8f7f3d1ab8cc3dc43cd061f12"
        .parse()
        .unwrap()
}

fn uncached_resource_addresses() -> [ResourceAddress; 2] {
    [
        "resource_c71b23ff336a5e0c05781b44dca0d0075543b564d0c254f92cb928b4ab9c9ec5",
        "resource_e04ebb967a75fa6a7e37fc8e25bb93f17eebf295c0d4b6d004499b4d45cd6014",
    ]
    .map(|address| address.parse().unwrap())
}

fn stealth_resource() -> Resource {
    Resource::new(
        ResourceType::Stealth,
        SubstateOwnerRule::None,
        ResourceAccessRules::new(),
        Metadata::from([("symbol", "STL".to_string())]),
        None,
        None,
        6,
        false,
    )
}

/// A wallet that has cached [`cached_resource_address`], with a second account that no resource but TARI is
/// associated with.
fn wallet_with_recipient() -> (ProvingNetwork, Wallet, AccountWithAddress) {
    let sets = (1..=2).map(ValidatorSetFixture::new).collect::<Vec<_>>();
    let network = ProvingNetwork::new(&sets, 0);
    let test = TestWithNetwork::with_network_trusting(network.clone(), IndexerTrust::Trust);
    test.sdk()
        .resources_api()
        .upsert_resource(&cached_resource_address(), &stealth_resource())
        .unwrap();
    let address = test.sdk().key_manager_api().derive_account_address(1).unwrap();
    let recipient = test
        .sdk()
        .accounts_api()
        .create_account(Some("recipient"), false, address)
        .unwrap();
    (network, test, recipient)
}

/// The wallet holds an output of `resource_address` paying `account` in `status`.
fn hold(test: &Wallet, account: &AccountWithAddress, resource_address: ResourceAddress, status: OutputStatus) {
    let (id, utxo) = test.owned_utxo_of_resource(account, &resource_address, 100, &random_key());
    let output = utxo.output().unwrap();
    test.sdk()
        .stealth_outputs_api()
        .add_output(&StealthOutputModel {
            owner_account: *account.account.component_address(),
            resource_address,
            commitment: id.into_commitment_bytes(),
            value: 100,
            sender_public_nonce: output.output.public_nonce,
            view_only_key_id: account.view_only_key_id(),
            owner_key_id: account.owner_key_id(),
            encrypted_data: output.output.encrypted_data.clone(),
            tag_byte: output.tag,
            memo: None,
            auth: output.auth.clone(),
            minimum_value_promise: 0,
            status,
            is_burnt: false,
            is_frozen: false,
            is_on_chain: status != OutputStatus::LockedUnconfirmed,
            is_condition_spendable: true,
            lock_id: None,
            proven_up: None,
            spent_version: None,
        })
        .unwrap();
}

fn is_associated(test: &Wallet, account: &ComponentAddress, resource_address: &ResourceAddress) -> bool {
    test.sdk()
        .accounts_api()
        .get_associated_stealth_resources(account)
        .unwrap()
        .contains(resource_address)
}

async fn associate_live_resources(test: &Wallet, account: &ComponentAddress) {
    AccountScanner::new(Notify::new(100), test.sdk().clone())
        .associate_live_stealth_resources(account)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_account_holding_an_unspent_output_of_a_resource_is_associated_with_it() {
    let (_, test, recipient) = wallet_with_recipient();
    let account = *recipient.account.component_address();
    hold(&test, &recipient, cached_resource_address(), OutputStatus::Unspent);
    assert!(!is_associated(&test, &account, &cached_resource_address()));

    associate_live_resources(&test, &account).await;

    assert!(is_associated(&test, &account, &cached_resource_address()));
}

/// A transfer statement paying an account in the wallet records the account's output before the transaction is
/// submitted, so the finalized transaction's diff shows the wallet nothing new.
#[tokio::test(flavor = "multi_thread")]
async fn an_account_paid_by_a_pending_transfer_statement_is_associated_with_its_resource() {
    let (_, test, recipient) = wallet_with_recipient();
    let account = *recipient.account.component_address();
    hold(
        &test,
        &recipient,
        cached_resource_address(),
        OutputStatus::LockedUnconfirmed,
    );

    associate_live_resources(&test, &account).await;

    assert!(is_associated(&test, &account, &cached_resource_address()));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_account_holding_only_spent_outputs_of_a_resource_is_not_associated_with_it() {
    let (_, test, recipient) = wallet_with_recipient();
    let account = *recipient.account.component_address();
    hold(&test, &recipient, cached_resource_address(), OutputStatus::Spent);

    associate_live_resources(&test, &account).await;

    assert!(!is_associated(&test, &account, &cached_resource_address()));
}

/// The first fetch fails, whichever resource it is for, so the other resource is associated only when the failure does
/// not end the call.
#[tokio::test(flavor = "multi_thread")]
async fn a_resource_the_wallet_cannot_fetch_does_not_hold_back_the_others() {
    let (network, test, recipient) = wallet_with_recipient();
    let account = *recipient.account.component_address();
    for resource_address in uncached_resource_addresses() {
        network.serve_substate(
            resource_address.into(),
            SubstateVersion::ZERO,
            stealth_resource().into(),
            None,
        );
        hold(&test, &recipient, resource_address, OutputStatus::Unspent);
    }
    network.fail_substate_queries(1);

    associate_live_resources(&test, &account).await;

    let associated = uncached_resource_addresses()
        .iter()
        .filter(|resource_address| is_associated(&test, &account, resource_address))
        .count();
    assert_eq!(
        associated, 1,
        "only the resource whose fetch failed is left unassociated"
    );

    associate_live_resources(&test, &account).await;

    for resource_address in uncached_resource_addresses() {
        assert!(
            is_associated(&test, &account, &resource_address),
            "the failed resource is associated on a later call"
        );
    }
}
