//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A wallet that verifies its indexer recovers a stealth output it does not hold only once the indexer proves that the
//! network holds the output, and stores it with the contents and frozen flag the proof shows. It holds a recovered
//! output as frozen only once the indexer proves that the network holds the output frozen. The outputs of a lookup are
//! proven in one fetch, and an output that is not proven waits longer after each attempt.

mod support;

use std::time::Duration;

use ootle_byte_type::ToByteType;
use tari_engine_types::{
    Utxo,
    substate::{SubstateId, SubstateValue},
};
use tari_indexer_client::types::SubstateProof;
use tari_ootle_common_types::{Epoch, SubstateVersion};
use tari_ootle_light_client::test_fixtures::{NETWORK, Signers, ValidatorSetFixture, substate_proof};
use tari_ootle_wallet_sdk::{
    IndexerTrust,
    models::{AccountWithAddress, OutputStatus, ProvenUp, StealthOutputModel, UtxoUnspent},
    storage::{QueuedUtxo, ReadableWalletStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore},
};
use tari_ootle_wallet_sdk_services::{account_monitor::AccountScanner, notify::Notify, utxo_scanner::UtxoRecovery};
use tari_template_lib::types::{
    Amount,
    UtxoAddress,
    UtxoId,
    constants::STEALTH_TARI_RESOURCE_ADDRESS,
    stealth::SpendAuthorization,
};

use crate::support::{ProvingNetwork, TestWithNetwork, random_keypair};

const VALUE: u64 = 1_234;
const VERSION: SubstateVersion = SubstateVersion::ZERO;

type Test = TestWithNetwork<ProvingNetwork>;

/// A wallet with one account, anchored on the validator set of epoch 2 and walked to that of epoch 5.
async fn wallet(indexer_trust: IndexerTrust) -> (Vec<ValidatorSetFixture>, ProvingNetwork, Test, AccountWithAddress) {
    let sets = (1..=5).map(ValidatorSetFixture::new).collect::<Vec<_>>();
    let network = ProvingNetwork::new(&sets, 0);
    let test = TestWithNetwork::without_accounts_trusting(network.clone(), indexer_trust);
    test.sdk().validator_set_api().sync().await.unwrap();
    network.set_latest(3);
    let trusted = test.sdk().validator_set_api().sync().await.unwrap();
    assert_eq!(trusted.epoch(), Epoch(5));
    let address = test.sdk().key_manager_api().next_account_address().unwrap();
    let account = test
        .sdk()
        .accounts_api()
        .create_account(Some("test"), false, address)
        .unwrap();
    (sets, network, test, account)
}

fn substate_id(id: UtxoId) -> SubstateId {
    SubstateId::Utxo(UtxoAddress::new(STEALTH_TARI_RESOURCE_ADDRESS, id))
}

fn frozen(utxo: &Utxo) -> Utxo {
    let mut utxo = utxo.clone();
    utxo.freeze();
    utxo
}

/// The indexer serves `id` holding `utxo`, proven by the committee of the latest trusted validator set.
fn serve_proven(sets: &[ValidatorSetFixture], network: &ProvingNetwork, id: UtxoId, utxo: &Utxo) {
    serve_proven_at(sets, network, id, VERSION, utxo);
}

/// The indexer serves `id` at `version` holding `utxo`, proven by the committee of the latest trusted validator set.
fn serve_proven_at(
    sets: &[ValidatorSetFixture],
    network: &ProvingNetwork,
    id: UtxoId,
    version: SubstateVersion,
    utxo: &Utxo,
) {
    let value = SubstateValue::Utxo(utxo.clone());
    let proof = substate_proof(&sets[4], &substate_id(id), version, &value);
    network.serve_substate(substate_id(id), version, value, Some(proof));
}

/// The wallet holds the output at `id` as spent, as a reported freeze's down of its version leaves it.
fn mark_spent(test: &Test, id: UtxoId) {
    test.sdk()
        .stealth_outputs_api()
        .update_utxo_status(
            &UtxoAddress::new(STEALTH_TARI_RESOURCE_ADDRESS, id),
            None,
            Some(OutputStatus::Spent),
            None,
        )
        .unwrap();
}

/// The indexer returns `utxo` to the full-UTXO lookup, and the wallet queues it for recovery.
fn queue(test: &Test, network: &ProvingNetwork, account: &AccountWithAddress, id: UtxoId, utxo: &Utxo) {
    network.index_utxo(id, utxo.clone());
    let output = utxo.output().unwrap();
    let item = (
        UtxoUnspent {
            tag: output.tag,
            public_nonce: output.output.public_nonce,
        },
        QueuedUtxo {
            account: *account.account.component_address(),
            unspent_at: None,
        },
    );
    test.store()
        .with_write_tx(|tx| tx.utxo_process_queue_extend(&STEALTH_TARI_RESOURCE_ADDRESS, [item]))
        .unwrap();
}

/// The indexer returns `utxo` to the full-UTXO lookup, and the wallet recovers it.
async fn recover(
    test: &Test,
    network: &ProvingNetwork,
    account: &AccountWithAddress,
    id: UtxoId,
    utxo: &Utxo,
) -> usize {
    queue(test, network, account, id, utxo);
    UtxoRecovery::new(test.sdk().clone())
        .process_utxo_validation_queue()
        .await
        .unwrap()
}

fn stored(test: &Test, id: UtxoId) -> StealthOutputModel {
    test.sdk()
        .stealth_outputs_api()
        .utxos_get_many(&STEALTH_TARI_RESOURCE_ADDRESS, None, None)
        .unwrap()
        .into_iter()
        .find(|output| output.commitment == id.into_commitment_bytes())
        .expect("the output is stored")
}

fn is_stored(test: &Test, id: UtxoId) -> bool {
    test.sdk()
        .stealth_outputs_api()
        .utxo_exists(&UtxoAddress::new(STEALTH_TARI_RESOURCE_ADDRESS, id))
        .unwrap()
}

fn queued(test: &Test) -> usize {
    test.store()
        .with_read_tx(|tx| tx.utxo_process_queue_fetch_batch(None, 1000))
        .unwrap()
        .len()
}

fn balance(test: &Test, account: &AccountWithAddress) -> Amount {
    test.sdk()
        .stealth_outputs_api()
        .get_unspent_balance_for_account_resource(account.account.component_address(), &STEALTH_TARI_RESOURCE_ADDRESS)
        .unwrap()
}

/// The wallet recovers `utxo` and so holds it.
async fn hold(
    sets: &[ValidatorSetFixture],
    network: &ProvingNetwork,
    test: &Test,
    account: &AccountWithAddress,
    id: UtxoId,
    utxo: &Utxo,
) {
    serve_proven(sets, network, id, utxo);
    assert_eq!(recover(test, network, account, id, utxo).await, 0);
    assert_eq!(stored(test, id).is_frozen, utxo.is_frozen());
}

/// The lookup returns `utxo` for an output the wallet does not hold, and the wallet leaves it queued without storing
/// it.
async fn assert_not_recovered(
    network: &ProvingNetwork,
    test: &Test,
    account: &AccountWithAddress,
    id: UtxoId,
    utxo: &Utxo,
) {
    assert_eq!(recover(test, network, account, id, utxo).await, 1);
    assert!(!is_stored(test, id));
    assert_eq!(balance(test, account), Amount::from_u64(0));
    assert_eq!(queued(test), 1, "an output that is not proven stays queued");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_output_without_a_proof_is_not_stored() {
    let (_, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    network.serve_substate(substate_id(id), VERSION, SubstateValue::Utxo(utxo.clone()), None);

    assert_not_recovered(&network, &test, &account, id, &utxo).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_output_the_network_holds_down_is_not_stored() {
    let (_, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    network.serve_down_substate(substate_id(id), VERSION, None);

    assert_not_recovered(&network, &test, &account, id, &utxo).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_output_proven_by_an_untrusted_committee_is_not_stored() {
    let (_, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    let impostors = ValidatorSetFixture::with_signers(NETWORK, 5, Signers::new(0xe0, 4), Signers::new(0xe1, 4));
    let value = SubstateValue::Utxo(utxo.clone());
    let proof = substate_proof(&impostors, &substate_id(id), VERSION, &value);
    network.serve_substate(substate_id(id), VERSION, value, Some(proof));

    assert_not_recovered(&network, &test, &account, id, &utxo).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_output_proven_with_another_tag_and_nonce_is_not_stored() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    let (_, other) = test.owned_utxo(&account, VALUE);
    serve_proven(&sets, &network, id, &other);

    assert_not_recovered(&network, &test, &account, id, &utxo).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unproven_output_stays_queued_until_it_is_proven() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    network.serve_substate(substate_id(id), VERSION, SubstateValue::Utxo(utxo.clone()), None);
    assert_not_recovered(&network, &test, &account, id, &utxo).await;

    serve_proven(&sets, &network, id, &utxo);
    let num_unresolved = UtxoRecovery::new(test.sdk().clone())
        .process_utxo_validation_queue()
        .await
        .unwrap();
    assert_eq!(num_unresolved, 0);
    assert_eq!(queued(&test), 0);
    assert_eq!(stored(&test, id).value, VALUE);
    assert_eq!(balance(&test, &account), Amount::from_u64(VALUE));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_output_is_stored_with_the_frozen_flag_the_proof_shows() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (unfrozen_id, unfrozen) = test.owned_utxo(&account, VALUE);
    serve_proven(&sets, &network, unfrozen_id, &unfrozen);
    let (frozen_id, held_frozen) = test.owned_utxo(&account, VALUE);
    serve_proven(&sets, &network, frozen_id, &frozen(&held_frozen));

    assert_eq!(
        recover(&test, &network, &account, unfrozen_id, &frozen(&unfrozen)).await,
        0
    );
    let output = stored(&test, unfrozen_id);
    assert!(!output.is_frozen);
    assert_eq!(output.value, VALUE);

    assert_eq!(recover(&test, &network, &account, frozen_id, &held_frozen).await, 0);
    assert!(stored(&test, frozen_id).is_frozen);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_recovered_output_is_stored_with_the_version_and_epoch_it_was_proven_up_at() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&sets, &network, &test, &account, id, &utxo).await;

    assert_eq!(
        stored(&test, id).proven_up,
        Some(ProvenUp {
            version: VERSION,
            epoch: Epoch(5),
        })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_spent_output_proven_up_at_a_later_version_is_held_unspent_again() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&sets, &network, &test, &account, id, &utxo).await;
    mark_spent(&test, id);

    serve_proven_at(&sets, &network, id, VERSION.next(), &frozen(&utxo));
    assert_eq!(recover(&test, &network, &account, id, &frozen(&utxo)).await, 0);
    let output = stored(&test, id);
    assert_eq!(output.status, OutputStatus::Unspent);
    assert!(output.is_frozen);
    assert_eq!(output.proven_up.unwrap().version, VERSION.next());
    assert_eq!(queued(&test), 0);
}

/// A proof of the version the wallet holds proven can predate the wallet's own spend of it, so it does not bring the
/// output back.
#[tokio::test(flavor = "multi_thread")]
async fn a_spent_output_proven_up_at_the_version_it_holds_proven_stays_spent() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&sets, &network, &test, &account, id, &utxo).await;
    mark_spent(&test, id);

    assert_eq!(recover(&test, &network, &account, id, &utxo).await, 0);
    assert_eq!(stored(&test, id).status, OutputStatus::Spent);
    assert_eq!(balance(&test, &account), Amount::from_u64(0));
    assert_eq!(queued(&test), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_spent_output_reported_unspent_without_a_proof_stays_spent() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&sets, &network, &test, &account, id, &utxo).await;
    mark_spent(&test, id);

    network.serve_substate(
        substate_id(id),
        VERSION.next(),
        SubstateValue::Utxo(frozen(&utxo)),
        None,
    );
    assert_eq!(recover(&test, &network, &account, id, &frozen(&utxo)).await, 0);
    let output = stored(&test, id);
    assert_eq!(output.status, OutputStatus::Spent);
    assert!(!output.is_frozen);
    assert_eq!(queued(&test), 0);
}

/// The wallet holds the output of `utxo` at `id` without a proof of any version, as it holds an output its own
/// transaction created.
fn hold_unproven(test: &Test, account: &AccountWithAddress, id: UtxoId, utxo: &Utxo) {
    let output = utxo.output().unwrap();
    test.sdk()
        .stealth_outputs_api()
        .add_output(&StealthOutputModel {
            owner_account: *account.account.component_address(),
            resource_address: STEALTH_TARI_RESOURCE_ADDRESS,
            commitment: id.into_commitment_bytes(),
            value: VALUE,
            sender_public_nonce: output.output.public_nonce,
            view_only_key_id: account.view_only_key_id(),
            owner_key_id: account.owner_key_id(),
            encrypted_data: output.output.encrypted_data.clone(),
            tag_byte: output.tag,
            memo: None,
            auth: output.auth.clone(),
            minimum_value_promise: 0,
            status: OutputStatus::Unspent,
            is_burnt: false,
            is_frozen: false,
            is_on_chain: true,
            is_condition_spendable: true,
            lock_id: None,
            proven_up: None,
            spent_version: None,
        })
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_held_output_without_a_proof_is_proven_when_it_is_reported() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold_unproven(&test, &account, id, &utxo);

    serve_proven(&sets, &network, id, &utxo);
    assert_eq!(recover(&test, &network, &account, id, &utxo).await, 0);
    assert_eq!(
        stored(&test, id).proven_up,
        Some(ProvenUp {
            version: VERSION,
            epoch: Epoch(5),
        })
    );
}

/// An output held without a proof, as a wallet that trusted its indexer received it, is proven once the wallet
/// verifies its indexer, without the indexer reporting it again.
#[tokio::test(flavor = "multi_thread")]
async fn a_held_output_without_a_proof_is_proven_without_being_reported() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold_unproven(&test, &account, id, &utxo);

    serve_proven(&sets, &network, id, &utxo);
    AccountScanner::new(Notify::new(1), test.sdk().clone())
        .prove_unproven_held_outputs()
        .await
        .unwrap();
    assert_eq!(
        stored(&test, id).proven_up,
        Some(ProvenUp {
            version: VERSION,
            epoch: Epoch(5),
        })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_spent_output_without_a_proof_is_not_proven() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold_unproven(&test, &account, id, &utxo);
    mark_spent(&test, id);

    serve_proven(&sets, &network, id, &utxo);
    AccountScanner::new(Notify::new(1), test.sdk().clone())
        .prove_unproven_held_outputs()
        .await
        .unwrap();
    assert!(stored(&test, id).proven_up.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_held_output_the_indexer_cannot_prove_is_settled_without_a_proof() {
    let (_, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold_unproven(&test, &account, id, &utxo);

    network.serve_substate(substate_id(id), VERSION, SubstateValue::Utxo(utxo.clone()), None);
    assert_eq!(recover(&test, &network, &account, id, &utxo).await, 0);
    let output = stored(&test, id);
    assert_eq!(output.status, OutputStatus::Unspent);
    assert!(output.proven_up.is_none());
    assert_eq!(queued(&test), 0);
}

/// A proof of a version the wallet's own transaction spent can predate that spend, so it does not bring the output
/// back, though the wallet holds an earlier version proven.
#[tokio::test(flavor = "multi_thread")]
async fn a_spent_output_proven_up_no_later_than_the_version_it_saw_spent_stays_spent() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&sets, &network, &test, &account, id, &utxo).await;
    test.store()
        .with_write_tx(|tx| {
            tx.stealth_outputs_mark_as_spent(&STEALTH_TARI_RESOURCE_ADDRESS, &id, VERSION.next().next())
        })
        .unwrap();

    serve_proven_at(&sets, &network, id, VERSION.next().next(), &utxo);
    assert_eq!(recover(&test, &network, &account, id, &utxo).await, 0);
    assert_eq!(stored(&test, id).status, OutputStatus::Spent);
    assert_eq!(balance(&test, &account), Amount::from_u64(0));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_held_output_needs_no_proof_that_it_exists() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&sets, &network, &test, &account, id, &utxo).await;

    let fetches = network.substate_fetches();
    assert_eq!(recover(&test, &network, &account, id, &utxo).await, 0);
    assert_eq!(network.substate_fetches(), fetches);
    assert_eq!(queued(&test), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_held_output_keeps_its_status_when_the_lookup_reports_other_contents() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&sets, &network, &test, &account, id, &utxo).await;

    let mut output = utxo.output().unwrap().clone();
    output.auth = SpendAuthorization::Key(random_keypair().1.to_byte_type());
    let reported = Utxo::new(output);
    assert_eq!(recover(&test, &network, &account, id, &reported).await, 0);
    assert_eq!(stored(&test, id).status, OutputStatus::Unspent);
    assert_eq!(balance(&test, &account), Amount::from_u64(VALUE));
}

/// The wallet holds `utxo` as not frozen, then recovers it again with the indexer reporting it frozen, serving
/// `value` with `proof`.
async fn assert_unproven_freeze_is_not_applied(
    sets: &[ValidatorSetFixture],
    network: &ProvingNetwork,
    test: &Test,
    account: &AccountWithAddress,
    id: UtxoId,
    utxo: &Utxo,
    value: SubstateValue,
    proof: Option<SubstateProof>,
) {
    hold(sets, network, test, account, id, utxo).await;
    network.serve_substate(substate_id(id), VERSION, value, proof);

    assert_eq!(recover(test, network, account, id, &frozen(utxo)).await, 0);
    assert!(
        !stored(test, id).is_frozen,
        "a held output stays spendable without a proof that it is frozen"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reported_freeze_without_a_proof_leaves_a_held_output_spendable() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    let value = SubstateValue::Utxo(frozen(&utxo));

    assert_unproven_freeze_is_not_applied(&sets, &network, &test, &account, id, &utxo, value, None).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reported_freeze_proven_by_an_untrusted_committee_leaves_a_held_output_spendable() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    let impostors = ValidatorSetFixture::with_signers(NETWORK, 5, Signers::new(0xe0, 4), Signers::new(0xe1, 4));
    let value = SubstateValue::Utxo(frozen(&utxo));
    let proof = substate_proof(&impostors, &substate_id(id), VERSION, &value);

    assert_unproven_freeze_is_not_applied(&sets, &network, &test, &account, id, &utxo, value, Some(proof)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reported_freeze_proven_unfrozen_leaves_a_held_output_spendable() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    let value = SubstateValue::Utxo(utxo.clone());
    let proof = substate_proof(&sets[4], &substate_id(id), VERSION, &value);

    assert_unproven_freeze_is_not_applied(&sets, &network, &test, &account, id, &utxo, value, Some(proof)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_proven_freeze_freezes_a_held_output() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&sets, &network, &test, &account, id, &utxo).await;

    serve_proven(&sets, &network, id, &frozen(&utxo));
    assert_eq!(recover(&test, &network, &account, id, &frozen(&utxo)).await, 0);
    assert!(stored(&test, id).is_frozen);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_output_whose_proof_could_not_be_fetched_stays_queued_until_it_is_proven() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    serve_proven(&sets, &network, id, &frozen(&utxo));
    network.fail_substate_fetches(1);

    assert_eq!(recover(&test, &network, &account, id, &frozen(&utxo)).await, 1);
    assert!(!is_stored(&test, id));

    let num_unresolved = UtxoRecovery::new(test.sdk().clone())
        .process_utxo_validation_queue()
        .await
        .unwrap();
    assert_eq!(num_unresolved, 0);
    assert!(stored(&test, id).is_frozen);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reported_unfreeze_needs_no_proof() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&sets, &network, &test, &account, id, &frozen(&utxo)).await;

    let fetches = network.substate_fetches();
    assert_eq!(recover(&test, &network, &account, id, &utxo).await, 0);
    assert!(!stored(&test, id).is_frozen);
    assert_eq!(network.substate_fetches(), fetches);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reported_freeze_of_an_output_held_frozen_needs_no_proof() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&sets, &network, &test, &account, id, &frozen(&utxo)).await;

    let fetches = network.substate_fetches();
    assert_eq!(recover(&test, &network, &account, id, &frozen(&utxo)).await, 0);
    assert!(stored(&test, id).is_frozen);
    assert_eq!(network.substate_fetches(), fetches);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_trusting_wallet_recovers_and_freezes_an_output_without_a_proof() {
    let (_, network, test, account) = wallet(IndexerTrust::Trust).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);

    assert_eq!(recover(&test, &network, &account, id, &utxo).await, 0);
    assert_eq!(recover(&test, &network, &account, id, &frozen(&utxo)).await, 0);
    assert!(stored(&test, id).is_frozen);
    assert_eq!(network.substate_fetches(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_outputs_of_a_lookup_are_proven_in_one_fetch() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let outputs = (0..3).map(|_| test.owned_utxo(&account, VALUE)).collect::<Vec<_>>();
    for (id, utxo) in &outputs {
        serve_proven(&sets, &network, *id, utxo);
        queue(&test, &network, &account, *id, utxo);
    }

    let fetches = network.substate_fetches();
    let num_unresolved = UtxoRecovery::new(test.sdk().clone())
        .process_utxo_validation_queue()
        .await
        .unwrap();
    assert_eq!(num_unresolved, 0);
    assert_eq!(network.substate_fetches(), fetches + 1);
    assert_eq!(balance(&test, &account), Amount::from_u64(3 * VALUE));
}

#[tokio::test(start_paused = true)]
async fn an_unproven_output_is_asked_for_again_after_a_doubling_wait() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    network.serve_substate(substate_id(id), VERSION, SubstateValue::Utxo(utxo.clone()), None);
    queue(&test, &network, &account, id, &utxo);
    let mut recovery = UtxoRecovery::new(test.sdk().clone());
    let fetches = network.substate_fetches();

    let mut process = async |advance_secs: u64, expected_fetches: usize| {
        tokio::time::advance(Duration::from_secs(advance_secs)).await;
        let num_unresolved = recovery.process_utxo_validation_queue().await.unwrap();
        assert_eq!(network.substate_fetches(), fetches + expected_fetches);
        num_unresolved
    };
    assert_eq!(process(0, 1).await, 1);
    assert_eq!(process(0, 1).await, 1, "the output waits before it is asked for again");
    assert_eq!(process(60, 2).await, 1);
    assert_eq!(process(60, 2).await, 1, "the wait doubles");
    assert_eq!(process(60, 3).await, 1);

    serve_proven(&sets, &network, id, &utxo);
    assert_eq!(process(240, 4).await, 0);
    assert_eq!(stored(&test, id).value, VALUE);
}

/// An output another sender made with the tag and nonce of `utxo`, which the wallet cannot spend.
fn copy_of(test: &Test, account: &AccountWithAddress, utxo: &Utxo) -> (UtxoId, Utxo) {
    let (id, _) = test.owned_utxo(account, 1);
    let mut output = utxo.output().unwrap().clone();
    output.auth = SpendAuthorization::Key(random_keypair().1.to_byte_type());
    (id, Utxo::new(output))
}

#[tokio::test(flavor = "multi_thread")]
async fn an_output_sharing_its_tag_and_nonce_with_another_is_recovered() {
    let (_sets, network, test, account) = wallet(IndexerTrust::Trust).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    let (copy_id, copy) = copy_of(&test, &account, &utxo);
    network.index_utxo(copy_id, copy);

    assert_eq!(recover(&test, &network, &account, id, &utxo).await, 0);
    assert_eq!(queued(&test), 0);
    assert_eq!(stored(&test, id).value, VALUE);
    assert!(!is_stored(&test, copy_id));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unproven_output_sharing_its_tag_and_nonce_with_a_proven_one_stays_queued_until_it_is_proven() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    let (copy_id, copy) = copy_of(&test, &account, &utxo);
    serve_proven(&sets, &network, copy_id, &copy);
    network.index_utxo(copy_id, copy);
    network.serve_substate(substate_id(id), VERSION, SubstateValue::Utxo(utxo.clone()), None);

    assert_eq!(recover(&test, &network, &account, id, &utxo).await, 1);
    assert_eq!(
        queued(&test),
        1,
        "the pair stays queued while one of its outputs is unproven"
    );
    assert!(!is_stored(&test, id));
    assert!(!is_stored(&test, copy_id));

    serve_proven(&sets, &network, id, &utxo);
    let num_unresolved = UtxoRecovery::new(test.sdk().clone())
        .process_utxo_validation_queue()
        .await
        .unwrap();
    assert_eq!(num_unresolved, 0);
    assert_eq!(queued(&test), 0);
    assert_eq!(stored(&test, id).value, VALUE);
    assert!(!is_stored(&test, copy_id));
}
