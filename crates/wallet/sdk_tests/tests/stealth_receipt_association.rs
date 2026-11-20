//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! An account that receives a stealth output in a finalized transaction is scanned for the output's resource from
//! then on, so the wallet sees later changes to the output that another wallet makes, such as the issuer freezing it.

mod support;

use tari_engine_types::{
    resource::Resource,
    substate::{Substate, SubstateDiff, SubstateId, SubstateValue},
};
use tari_ootle_common_types::SubstateVersion;
use tari_ootle_light_client::test_fixtures::ValidatorSetFixture;
use tari_ootle_transaction::TransactionId;
use tari_ootle_wallet_sdk::IndexerTrust;
use tari_ootle_wallet_sdk_services::{account_monitor::AccountScanner, notify::Notify};
use tari_template_lib::types::{
    Amount,
    Metadata,
    ResourceAddress,
    ResourceType,
    SubstateOwnerRule,
    UtxoAddress,
    access_rules::ResourceAccessRules,
};

use crate::support::{ProvingNetwork, TestWithNetwork, random_key};

fn stealth_resource_address() -> ResourceAddress {
    "resource_38defd897ba069aff5038b8045fdbea5bbd94ed8f7f3d1ab8cc3dc43cd061f12"
        .parse()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_account_paid_a_stealth_output_of_a_new_resource_is_scanned_for_that_resource() {
    let sets = (1..=2).map(ValidatorSetFixture::new).collect::<Vec<_>>();
    let network = ProvingNetwork::new(&sets, 0);
    let test = TestWithNetwork::with_network_trusting(network, IndexerTrust::Trust);
    let resource_address = stealth_resource_address();
    test.sdk()
        .resources_api()
        .upsert_resource(
            &resource_address,
            &Resource::new(
                ResourceType::Stealth,
                SubstateOwnerRule::None,
                ResourceAccessRules::new(),
                Metadata::from([("symbol", "STL".to_string())]),
                None,
                None,
                6,
                false,
            ),
        )
        .unwrap();
    let accounts_api = test.sdk().accounts_api();
    let recipient_address = test.sdk().key_manager_api().derive_account_address(1).unwrap();
    let recipient = accounts_api
        .create_account(Some("recipient"), false, recipient_address)
        .unwrap();
    let recipient_account = *recipient.account.component_address();
    assert!(
        !accounts_api
            .get_associated_stealth_resources(&recipient_account)
            .unwrap()
            .contains(&resource_address),
        "the account starts without the resource"
    );

    let (id, utxo) = test.owned_utxo_of_resource(&recipient, &resource_address, 100, &random_key());
    let mut diff = SubstateDiff::new();
    diff.up(
        SubstateId::Utxo(UtxoAddress::new(resource_address, id)),
        Substate::new(SubstateVersion::ZERO, SubstateValue::Utxo(utxo)),
    );

    AccountScanner::new(Notify::new(100), test.sdk().clone())
        .process_result(TransactionId::default(), &diff, None)
        .await
        .unwrap();

    assert_eq!(
        test.sdk()
            .stealth_outputs_api()
            .get_unspent_balance_for_account_resource(&recipient_account, &resource_address)
            .unwrap(),
        Amount::from(100u64),
        "the wallet holds the received output"
    );
    assert!(
        accounts_api
            .get_associated_stealth_resources(&recipient_account)
            .unwrap()
            .contains(&resource_address),
        "the receiving account is scanned for the resource"
    );
}
