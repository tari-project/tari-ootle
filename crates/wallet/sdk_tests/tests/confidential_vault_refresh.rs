//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Reconciling locally unspent confidential outputs when a refreshed vault no longer holds them.

mod support;

use ootle_byte_type::ToByteType;
use tari_crypto::{commitment::HomomorphicCommitmentFactory, ristretto::RistrettoPublicKey};
use tari_engine_types::{
    component::{Component, ComponentBody, ComponentHeader},
    confidential_output::ConfidentialOutput,
    crypto::{OutputBody, get_commitment_factory},
    resource::Resource,
    resource_container::ResourceContainer,
    substate::{Substate, SubstateDiff, SubstateId, SubstateValue},
    vault::Vault,
};
use tari_ootle_common_types::{Epoch, SubstateVersion};
use tari_ootle_light_client::test_fixtures::{ValidatorSetFixture, substate_proof};
use tari_ootle_transaction::TransactionId;
use tari_ootle_wallet_sdk::{
    IndexerTrust,
    apis::confidential_outputs::ConfidentialOutputsApiError,
    models::{ConfidentialOutputModel, KeyBranch, KeyId, OutputStatus},
};
use tari_ootle_wallet_sdk_services::{account_monitor::AccountScanner, notify::Notify};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::types::{
    Amount,
    ComponentAddress,
    ConfidentialOutputAddress,
    EncryptedData,
    Metadata,
    ResourceAddress,
    ResourceType,
    SubstateOwnerRule,
    VaultId,
    access_rules::{ComponentAccessRules, ResourceAccessRules},
    crypto::PedersenCommitmentBytes,
};

use crate::support::{ProvingNetwork, Test, TestSdkSpec, TestWithNetwork, random_key, random_keypair};

const VERSION: SubstateVersion = SubstateVersion::new(3);

fn account_address() -> ComponentAddress {
    Test::test_account_address()
}

fn confidential_resource_address() -> ResourceAddress {
    "resource_7cbfe29101c24924b1b6ccefbfff98986d648622272ae24f7585dab5eeeeeeee"
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

fn confidential_vault_value(commitments: &[PedersenCommitmentBytes]) -> SubstateValue {
    Vault::new(ResourceContainer::confidential(
        confidential_resource_address(),
        commitments.iter().copied(),
        Amount::zero(),
    ))
    .into()
}

fn serve_proven_at(
    sets: &[ValidatorSetFixture],
    network: &ProvingNetwork,
    id: SubstateId,
    version: SubstateVersion,
    value: SubstateValue,
) {
    let proof = substate_proof(sets.last().unwrap(), &id, version, &value);
    network.serve_substate(id, version, value, Some(proof));
}

async fn verifying_wallet() -> (
    Vec<ValidatorSetFixture>,
    ProvingNetwork,
    TestWithNetwork<ProvingNetwork>,
) {
    let sets = (1..=3).map(ValidatorSetFixture::new).collect::<Vec<_>>();
    let network = ProvingNetwork::new(&sets, 1);
    let test = TestWithNetwork::without_accounts_trusting(network.clone(), IndexerTrust::Verify);
    let trusted = test.sdk().validator_set_api().sync().await.unwrap();
    assert_eq!(trusted.epoch(), Epoch(3));

    test.sdk()
        .accounts_api()
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
    test.sdk()
        .resources_api()
        .upsert_resource(
            &confidential_resource_address(),
            &Resource::new(
                ResourceType::Confidential,
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
    (sets, network, test)
}

fn scanner(test: &TestWithNetwork<ProvingNetwork>) -> AccountScanner<TestSdkSpec<ProvingNetwork>> {
    AccountScanner::new(Notify::new(8), test.sdk().clone())
}

fn account_view_key(test: &TestWithNetwork<ProvingNetwork>) -> RistrettoPublicKey {
    test.sdk()
        .key_manager_api()
        .get_public_key(KeyId::derived(KeyBranch::ViewOnlyKey, 0))
        .unwrap()
        .public_key
}

fn confidential_output(
    test: &TestWithNetwork<ProvingNetwork>,
    view_key: &RistrettoPublicKey,
    amount: u64,
) -> (PedersenCommitmentBytes, SubstateValue) {
    let mask = random_key();
    let (nonce, public_nonce) = random_keypair();
    let commitment = get_commitment_factory().commit_value(&mask, amount).to_byte_type();
    let encrypted_data = test
        .sdk()
        .confidential_crypto_api()
        .encrypt_value_and_mask(amount, &mask, view_key, &nonce, None)
        .unwrap();
    let output = ConfidentialOutput {
        output: OutputBody {
            public_nonce: public_nonce.to_byte_type(),
            encrypted_data,
            minimum_value_promise: 0,
            viewable_balance: None,
        },
        is_frozen: false,
    };
    (commitment, output.into())
}

fn output_id(commitment: PedersenCommitmentBytes) -> SubstateId {
    ConfidentialOutputAddress::new(confidential_resource_address(), commitment).into()
}

fn confidential_balance(test: &TestWithNetwork<ProvingNetwork>, vault: &VaultId) -> Amount {
    test.sdk().accounts_api().get_vault(vault).unwrap().confidential_balance
}

fn output_status(test: &TestWithNetwork<ProvingNetwork>, commitment: PedersenCommitmentBytes) -> Option<OutputStatus> {
    test.sdk()
        .confidential_outputs_api()
        .outputs_get_many(&confidential_resource_address(), Some(&account_address()), None)
        .unwrap()
        .into_iter()
        .find(|o| o.commitment == commitment)
        .map(|o| o.status)
}

#[tokio::test]
async fn an_absent_confidential_output_is_marked_spent_when_the_vault_advances() {
    let (sets, network, test) = verifying_wallet().await;
    let vault = vault_id(1);
    let view_key = account_view_key(&test);
    let (c1, out1) = confidential_output(&test, &view_key, 123);
    let (c2, out2) = confidential_output(&test, &view_key, 77);

    serve_proven_at(
        &sets,
        &network,
        account_address().into(),
        VERSION,
        account_value(&[vault]),
    );
    serve_proven_at(
        &sets,
        &network,
        vault.into(),
        VERSION,
        confidential_vault_value(&[c1, c2]),
    );
    serve_proven_at(&sets, &network, output_id(c1), SubstateVersion::ZERO, out1);
    serve_proven_at(&sets, &network, output_id(c2), SubstateVersion::ZERO, out2);

    assert!(scanner(&test).refresh_account(account_address()).await.unwrap());
    assert_eq!(confidential_balance(&test, &vault), Amount::from(200u64));
    assert_eq!(output_status(&test, c1), Some(OutputStatus::Unspent));
    assert_eq!(output_status(&test, c2), Some(OutputStatus::Unspent));

    // `c1` leaves the vault externally (e.g. spent from another wallet instance or recalled by the issuer).
    let v_next = SubstateVersion::new(VERSION.as_u64() + 1);
    serve_proven_at(&sets, &network, vault.into(), v_next, confidential_vault_value(&[c2]));

    assert!(scanner(&test).refresh_account(account_address()).await.unwrap());
    assert_eq!(confidential_balance(&test, &vault), Amount::from(77u64));
    assert_eq!(output_status(&test, c1), Some(OutputStatus::Spent));
    assert_eq!(output_status(&test, c2), Some(OutputStatus::Unspent));

    // Spending more than the remaining `c2` output fails instead of selecting the already-spent `c1`.
    let lock_too_much = test.new_lock().keep_locked();
    let err = test
        .sdk()
        .confidential_outputs_api()
        .lock_outputs_by_amount(lock_too_much, &vault, 78u64)
        .unwrap_err();
    assert!(matches!(err, ConfidentialOutputsApiError::InsufficientFunds));
    test.sdk().locks_api().release_lock(lock_too_much).unwrap();

    let lock_remaining = test.new_lock().keep_locked();
    let (locked_outputs, locked_total) = test
        .sdk()
        .confidential_outputs_api()
        .lock_outputs_by_amount(lock_remaining, &vault, 77u64)
        .unwrap();
    assert_eq!(locked_total, Amount::from(77u64));
    assert_eq!(locked_outputs.len(), 1);
    assert_eq!(locked_outputs[0].commitment, c2);
    test.sdk().locks_api().release_lock(lock_remaining).unwrap();

    // Once `c2` also leaves the vault, the confidential balance drops to zero.
    let v_empty = SubstateVersion::new(VERSION.as_u64() + 2);
    serve_proven_at(&sets, &network, vault.into(), v_empty, confidential_vault_value(&[]));

    assert!(scanner(&test).refresh_account(account_address()).await.unwrap());
    assert_eq!(confidential_balance(&test, &vault), Amount::zero());
    assert_eq!(output_status(&test, c2), Some(OutputStatus::Spent));
}

#[tokio::test]
async fn a_stale_vault_version_does_not_mark_newer_confidential_outputs_spent() {
    let (sets, network, test) = verifying_wallet().await;
    let vault = vault_id(1);
    let view_key = account_view_key(&test);
    let (c1, out1) = confidential_output(&test, &view_key, 123);
    let (c2, out2) = confidential_output(&test, &view_key, 77);

    let v_newer = SubstateVersion::new(VERSION.as_u64() + 1);
    serve_proven_at(
        &sets,
        &network,
        account_address().into(),
        v_newer,
        account_value(&[vault]),
    );
    serve_proven_at(
        &sets,
        &network,
        vault.into(),
        v_newer,
        confidential_vault_value(&[c1, c2]),
    );
    serve_proven_at(&sets, &network, output_id(c1), SubstateVersion::ZERO, out1);
    serve_proven_at(&sets, &network, output_id(c2), SubstateVersion::ZERO, out2);

    assert!(scanner(&test).refresh_account(account_address()).await.unwrap());
    assert_eq!(confidential_balance(&test, &vault), Amount::from(200u64));

    // A lagging indexer serving an older vault version that predates `c2` must not mark `c2` as spent.
    serve_proven_at(&sets, &network, vault.into(), VERSION, confidential_vault_value(&[c1]));
    assert!(!scanner(&test).refresh_account(account_address()).await.unwrap());
    assert_eq!(confidential_balance(&test, &vault), Amount::from(200u64));
    assert_eq!(output_status(&test, c2), Some(OutputStatus::Unspent));

    // Likewise, an out-of-order older transaction diff must not mark `c2` as spent.
    let mut stale_diff = SubstateDiff::new();
    stale_diff.up(vault.into(), Substate::new(VERSION, confidential_vault_value(&[c1])));
    scanner(&test)
        .process_result(TransactionId::default(), &stale_diff, None)
        .await
        .unwrap();
    assert_eq!(confidential_balance(&test, &vault), Amount::from(200u64));
    assert_eq!(output_status(&test, c2), Some(OutputStatus::Unspent));
}

#[tokio::test]
async fn locked_outputs_are_preserved_until_released_and_then_reconciled() {
    let (sets, network, test) = verifying_wallet().await;
    let vault = vault_id(1);
    let view_key = account_view_key(&test);
    let (c1, out1) = confidential_output(&test, &view_key, 123);

    serve_proven_at(
        &sets,
        &network,
        account_address().into(),
        VERSION,
        account_value(&[vault]),
    );
    serve_proven_at(&sets, &network, vault.into(), VERSION, confidential_vault_value(&[c1]));
    serve_proven_at(&sets, &network, output_id(c1), SubstateVersion::ZERO, out1);

    assert!(scanner(&test).refresh_account(account_address()).await.unwrap());
    assert_eq!(confidential_balance(&test, &vault), Amount::from(123u64));

    // Lock `c1` (`LockedForSpend`) and insert a pending change output (`LockedUnconfirmed`).
    let lock_id = test.new_lock().keep_locked();
    test.sdk()
        .confidential_outputs_api()
        .lock_outputs_by_amount(lock_id, &vault, 100u64)
        .unwrap();
    let pending_change_commitment = get_commitment_factory().commit_value(&random_key(), 23).to_byte_type();
    test.sdk()
        .confidential_outputs_api()
        .add_output(ConfidentialOutputModel {
            account_address: account_address(),
            vault_id: vault,
            commitment: pending_change_commitment,
            value: Amount::from(23u64),
            sender_public_nonce: None,
            view_only_key_id: KeyId::derived(KeyBranch::ViewOnlyKey, 0),
            owner_key_id: Some(KeyId::derived(KeyBranch::Account, 0)),
            encrypted_data: EncryptedData::try_from(vec![0u8; EncryptedData::min_size()]).unwrap(),
            public_asset_tag: None,
            memo: None,
            status: OutputStatus::LockedUnconfirmed,
            lock_id: Some(lock_id),
        })
        .unwrap();

    // Vault advances on-chain to empty while `lock_id` is still active.
    let v_next = SubstateVersion::new(VERSION.as_u64() + 1);
    serve_proven_at(&sets, &network, vault.into(), v_next, confidential_vault_value(&[]));
    scanner(&test).refresh_account(account_address()).await.unwrap();

    assert_eq!(output_status(&test, c1), Some(OutputStatus::LockedForSpend));
    assert_eq!(
        output_status(&test, pending_change_commitment),
        Some(OutputStatus::LockedUnconfirmed)
    );

    // If `lock_id` is released without finalizing (e.g. lock timeout), `c1` temporarily reverts to `Unspent` and the
    // next refresh at the same known vault version reconciles it to `Spent`.
    test.sdk().locks_api().release_lock(lock_id).unwrap();
    assert_eq!(output_status(&test, pending_change_commitment), None);
    assert_eq!(output_status(&test, c1), Some(OutputStatus::Unspent));

    assert!(scanner(&test).refresh_account(account_address()).await.unwrap());
    assert_eq!(output_status(&test, c1), Some(OutputStatus::Spent));
    assert_eq!(confidential_balance(&test, &vault), Amount::zero());
}

#[tokio::test]
async fn a_recalled_output_returned_to_the_vault_becomes_unspent_again() {
    let (sets, network, test) = verifying_wallet().await;
    let vault = vault_id(1);
    let view_key = account_view_key(&test);
    let (c1, out1) = confidential_output(&test, &view_key, 123);

    serve_proven_at(
        &sets,
        &network,
        account_address().into(),
        VERSION,
        account_value(&[vault]),
    );
    serve_proven_at(&sets, &network, vault.into(), VERSION, confidential_vault_value(&[c1]));
    serve_proven_at(&sets, &network, output_id(c1), SubstateVersion::ZERO, out1);

    assert!(scanner(&test).refresh_account(account_address()).await.unwrap());
    assert_eq!(confidential_balance(&test, &vault), Amount::from(123u64));
    assert_eq!(output_status(&test, c1), Some(OutputStatus::Unspent));

    // Issuer recalls `c1` out of the vault at `VERSION + 1`.
    let v_recalled = SubstateVersion::new(VERSION.as_u64() + 1);
    serve_proven_at(&sets, &network, vault.into(), v_recalled, confidential_vault_value(&[]));
    assert!(scanner(&test).refresh_account(account_address()).await.unwrap());
    assert_eq!(confidential_balance(&test, &vault), Amount::zero());
    assert_eq!(output_status(&test, c1), Some(OutputStatus::Spent));

    // A lagging snapshot at `VERSION` must not resurrect `c1` while `VERSION + 1` is known.
    serve_proven_at(&sets, &network, vault.into(), VERSION, confidential_vault_value(&[c1]));
    assert!(!scanner(&test).refresh_account(account_address()).await.unwrap());
    assert_eq!(confidential_balance(&test, &vault), Amount::zero());
    assert_eq!(output_status(&test, c1), Some(OutputStatus::Spent));

    // Issuer re-deposits the recalled bucket back into the vault at `VERSION + 2`.
    let v_returned = SubstateVersion::new(VERSION.as_u64() + 2);
    serve_proven_at(
        &sets,
        &network,
        vault.into(),
        v_returned,
        confidential_vault_value(&[c1]),
    );
    assert!(scanner(&test).refresh_account(account_address()).await.unwrap());
    assert_eq!(confidential_balance(&test, &vault), Amount::from(123u64));
    assert_eq!(output_status(&test, c1), Some(OutputStatus::Unspent));

    let lock_id = test.new_lock().keep_locked();
    let (locked, total) = test
        .sdk()
        .confidential_outputs_api()
        .lock_outputs_by_amount(lock_id, &vault, 123u64)
        .unwrap();
    assert_eq!(total, Amount::from(123u64));
    assert_eq!(locked.len(), 1);
    assert_eq!(locked[0].commitment, c1);
    test.sdk().locks_api().release_lock(lock_id).unwrap();
}
