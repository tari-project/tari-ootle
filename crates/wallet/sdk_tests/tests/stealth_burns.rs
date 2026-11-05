//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A wallet holds a stealth output that the resource issuer burnt as burnt: a final status that counts toward no
//! balance and that no later report changes. A burn leaves the UTXO up without an output, so every read of the UTXO
//! after the burn shows it burnt.

mod support;

use tari_engine_types::{
    Utxo,
    substate::{Substate, SubstateDiff, SubstateId, SubstateValue},
};
use tari_ootle_common_types::{
    Epoch,
    NumPreshards,
    StateVersion,
    SubstateVersion,
    VersionedSubstateIdRef,
    shard::Shard,
};
use tari_ootle_light_client::test_fixtures::{ValidatorSetFixture, substate_down_proof, substate_proof};
use tari_ootle_transaction::TransactionId;
use tari_ootle_wallet_sdk::{
    IndexerTrust,
    models::{
        AccountWithAddress,
        BalanceChangeSource,
        EndOfShard,
        OutputStatus,
        ProvenUp,
        StartOfShard,
        StealthOutputModel,
        UtxoBurnt,
        UtxoSpent,
        UtxoUnspent,
        UtxoUpdatePayload,
        WalletEvent,
        WalletUtxoUpdate,
    },
    storage::{QueuedUtxo, ReadableWalletStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore},
};
use tari_ootle_wallet_sdk_services::{
    account_monitor::AccountScanner,
    notify::Notify,
    utxo_scanner::{StealthUtxoScannerWorker, UtxoRecovery, UtxoScanRoundStats},
};
use tari_template_lib::types::{Amount, UtxoAddress, UtxoId, constants::STEALTH_TARI_RESOURCE_ADDRESS};

use crate::support::{ProvingNetwork, Test, TestWithNetwork};

const VALUE: u64 = 1_234;
/// The version of the output the wallet holds.
const VERSION: SubstateVersion = SubstateVersion::ZERO;
/// The version a burn of [`VERSION`] brings up, without an output.
const BURNT_VERSION: SubstateVersion = SubstateVersion::new(1);
const SCANNED_TO: StateVersion = StateVersion::new(7);

type Wallet = TestWithNetwork<ProvingNetwork>;

/// A wallet holding the test account, anchored on the validator set of epoch 2 and walked to that of epoch 5.
async fn wallet(indexer_trust: IndexerTrust) -> (Vec<ValidatorSetFixture>, ProvingNetwork, Wallet, AccountWithAddress) {
    let sets = (1..=5).map(ValidatorSetFixture::new).collect::<Vec<_>>();
    let network = ProvingNetwork::new(&sets, 0);
    let test = TestWithNetwork::with_network_trusting(network.clone(), indexer_trust);
    test.sdk().validator_set_api().sync().await.unwrap();
    network.set_latest(3);
    let trusted = test.sdk().validator_set_api().sync().await.unwrap();
    assert_eq!(trusted.epoch(), Epoch(5));
    let account = test
        .sdk()
        .accounts_api()
        .get_account_by_address(&Test::test_account_address())
        .unwrap();
    (sets, network, test, account)
}

fn address(id: UtxoId) -> UtxoAddress {
    UtxoAddress::new(STEALTH_TARI_RESOURCE_ADDRESS, id)
}

fn substate_id(id: UtxoId) -> SubstateId {
    SubstateId::Utxo(address(id))
}

fn shard_of(id: UtxoId) -> Shard {
    VersionedSubstateIdRef::new(&substate_id(id), VERSION).to_shard(NumPreshards::P256)
}

fn burnt(utxo: &Utxo) -> Utxo {
    let mut utxo = utxo.clone();
    utxo.burn();
    utxo
}

/// The wallet holds `utxo` in `status`, proven up at [`VERSION`] in `proven_up_epoch`.
fn hold(
    test: &Wallet,
    account: &AccountWithAddress,
    id: UtxoId,
    utxo: &Utxo,
    proven_up_epoch: Option<u64>,
    status: OutputStatus,
) {
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
            status,
            is_burnt: false,
            is_frozen: false,
            is_on_chain: true,
            is_condition_spendable: true,
            lock_id: None,
            proven_up: proven_up_epoch.map(|epoch| ProvenUp {
                version: VERSION,
                epoch: Epoch(epoch),
            }),
            spent_version: None,
        })
        .unwrap();
}

/// The indexer answers each scan of the shard of `id` from below [`SCANNED_TO`] with `update`, flagging the shard as
/// possibly lacking spends below the cursor when `spends_incomplete` is set.
fn answer_scans(network: &ProvingNetwork, id: UtxoId, update: Option<WalletUtxoUpdate>, spends_incomplete: bool) {
    let shard = shard_of(id);
    network.answer_utxo_streams(move |cursors, _| {
        if !cursors.iter().any(|(s, cursor)| *s == shard && *cursor < SCANNED_TO) {
            return vec![];
        }
        vec![UtxoUpdatePayload {
            sos: Some(StartOfShard {
                shard,
                max_state_version: SCANNED_TO,
                has_more: false,
                spends_incomplete,
            }),
            update: update.clone(),
            eos: Some(EndOfShard {
                max_state_version: SCANNED_TO,
            }),
        }]
    });
}

fn report_burnt(network: &ProvingNetwork, id: UtxoId) {
    answer_scans(
        network,
        id,
        Some(WalletUtxoUpdate::Burnt(UtxoBurnt { id, version: VERSION })),
        false,
    );
}

/// The indexer serves [`VERSION`] of `id` as gone down, proven up in the anchor's epoch and down in the latest trusted
/// one.
fn serve_proven_down(sets: &[ValidatorSetFixture], network: &ProvingNetwork, id: UtxoId) {
    let substate_id = substate_id(id);
    let proof = substate_down_proof(&sets[1], &sets[4], &substate_id, VERSION);
    network.serve_down_version(substate_id, VERSION, Some(proof));
}

/// The indexer serves `id` at [`BURNT_VERSION`] holding `utxo`, proven by the committee of the latest trusted validator
/// set when `proven`.
fn serve_latest(sets: &[ValidatorSetFixture], network: &ProvingNetwork, id: UtxoId, utxo: &Utxo, proven: bool) {
    let value = SubstateValue::Utxo(utxo.clone());
    let proof = proven.then(|| substate_proof(&sets[4], &substate_id(id), BURNT_VERSION, &value));
    network.serve_substate(substate_id(id), BURNT_VERSION, value, proof);
}

async fn scan_notifying(test: &Wallet, notify: Notify<WalletEvent>) -> UtxoScanRoundStats {
    let (_join, scanner) = StealthUtxoScannerWorker::new(test.sdk().clone(), notify).spawn();
    scanner
        .scan(Test::test_account_address(), STEALTH_TARI_RESOURCE_ADDRESS)
        .await
        .unwrap()
}

async fn scan(test: &Wallet) -> UtxoScanRoundStats {
    scan_notifying(test, Notify::new(100)).await
}

fn stored(test: &Wallet, id: UtxoId) -> StealthOutputModel {
    test.sdk()
        .stealth_outputs_api()
        .get_utxo(&address(id))
        .unwrap()
        .expect("the output is stored")
}

fn balance(test: &Wallet, account: &AccountWithAddress) -> Amount {
    test.sdk()
        .stealth_outputs_api()
        .get_unspent_balance_for_account_resource(account.account.component_address(), &STEALTH_TARI_RESOURCE_ADDRESS)
        .unwrap()
}

fn assert_burnt(test: &Wallet, account: &AccountWithAddress, id: UtxoId) {
    let output = stored(test, id);
    assert_eq!(output.status, OutputStatus::Burnt);
    assert!(output.is_burnt);
    assert_eq!(output.lock_id, None);
    assert_eq!(balance(test, account), Amount::zero());
}

/// The number of [`WalletEvent::UtxoBurnt`] events received, asserting that no [`WalletEvent::UtxoSpent`] is.
fn count_burnt_events(events: &mut tokio::sync::broadcast::Receiver<WalletEvent>) -> usize {
    let mut burnt_events = 0;
    while let Ok(event) = events.try_recv() {
        assert!(!matches!(event, WalletEvent::UtxoSpent(_)), "a burn is not a spend");
        if matches!(event, WalletEvent::UtxoBurnt(_)) {
            burnt_events += 1;
        }
    }
    burnt_events
}

/// The indexer returns `reported` to the full-UTXO lookup of the output at `id`, and the wallet runs its recovery.
async fn recover_reported(
    test: &Wallet,
    network: &ProvingNetwork,
    account: &AccountWithAddress,
    id: UtxoId,
    reported: &Utxo,
) {
    network.index_utxo(id, reported.clone());
    let output = reported.output().unwrap();
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

    let unresolved = UtxoRecovery::new(test.sdk().clone())
        .process_utxo_validation_queue()
        .await
        .unwrap();
    assert_eq!(unresolved, 0);
    let queued = test
        .store()
        .with_read_tx(|tx| tx.utxo_process_queue_fetch_batch(None, 1000))
        .unwrap()
        .len();
    assert_eq!(queued, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_trusting_wallet_records_a_reported_burn() {
    let (_, network, test, account) = wallet(IndexerTrust::Trust).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&test, &account, id, &utxo, None, OutputStatus::Unspent);
    report_burnt(&network, id);
    let notify = Notify::new(100);
    let mut events = notify.subscribe();

    let stats = scan_notifying(&test, notify).await;
    assert_eq!(stats.num_burnt, 1);
    assert_eq!(stats.num_spent, 0);
    assert_burnt(&test, &account, id);
    assert_eq!(count_burnt_events(&mut events), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_burnt_output_stays_burnt_when_reported_spent() {
    let (_, network, test, account) = wallet(IndexerTrust::Trust).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&test, &account, id, &utxo, None, OutputStatus::Burnt);
    answer_scans(
        &network,
        id,
        Some(WalletUtxoUpdate::Spent(UtxoSpent {
            id,
            version: BURNT_VERSION,
        })),
        false,
    );

    let stats = scan(&test).await;
    assert_eq!(stats.num_spent, 0);
    assert_eq!(stored(&test, id).status, OutputStatus::Burnt);
}

/// An issuer can burn an output that a pending transaction of the wallet locked. The burn releases it from the lock,
/// so releasing the lock once the transaction fails leaves it burnt.
#[tokio::test(flavor = "multi_thread")]
async fn a_burn_of_an_output_locked_for_spend_releases_it_from_the_lock() {
    let (_, network, test, account) = wallet(IndexerTrust::Trust).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&test, &account, id, &utxo, None, OutputStatus::Unspent);
    let lock = test.new_lock();
    test.sdk()
        .stealth_outputs_api()
        .lock_specific_outputs(
            account.account.component_address(),
            &STEALTH_TARI_RESOURCE_ADDRESS,
            lock.id(),
            &[address(id)],
        )
        .unwrap();
    assert_eq!(stored(&test, id).status, OutputStatus::LockedForSpend);
    report_burnt(&network, id);

    let stats = scan(&test).await;
    assert_eq!(stats.num_burnt, 1);
    assert_burnt(&test, &account, id);

    lock.release();
    assert_burnt(&test, &account, id);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_verifying_wallet_records_a_burn_proven_by_a_read_of_the_burnt_utxo() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&test, &account, id, &utxo, Some(3), OutputStatus::Unspent);
    serve_proven_down(&sets, &network, id);
    serve_latest(&sets, &network, id, &burnt(&utxo), true);
    report_burnt(&network, id);

    let stats = scan(&test).await;
    assert_eq!(stats.num_burnt, 1);
    assert_eq!(stats.num_spent, 0);
    assert_burnt(&test, &account, id);
    assert_eq!(network.substate_fetches(), 1, "the burnt UTXO is read once");
}

/// A down proof shows the version gone, not that a burn took it down, so a burn the read does not prove is recorded as
/// the spend the down proof shows.
#[tokio::test(flavor = "multi_thread")]
async fn a_verifying_wallet_records_a_reported_burn_spent_when_the_burnt_utxo_is_not_proven() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&test, &account, id, &utxo, Some(3), OutputStatus::Unspent);
    serve_proven_down(&sets, &network, id);
    serve_latest(&sets, &network, id, &burnt(&utxo), false);
    report_burnt(&network, id);

    let stats = scan(&test).await;
    assert_eq!(stats.num_burnt, 0);
    assert_eq!(stats.num_spent, 1);
    let output = stored(&test, id);
    assert_eq!(output.status, OutputStatus::Spent);
    assert!(!output.is_burnt);
    assert_eq!(balance(&test, &account), Amount::zero());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_verifying_wallet_keeps_an_output_reported_burnt_without_a_proof_that_it_went_down() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&test, &account, id, &utxo, Some(3), OutputStatus::Unspent);
    network.serve_down_version(substate_id(id), VERSION, None);
    serve_latest(&sets, &network, id, &burnt(&utxo), true);
    report_burnt(&network, id);

    let stats = scan(&test).await;
    assert_eq!(stats.num_burnt, 0);
    assert_eq!(stats.num_spent, 0);
    assert_eq!(stored(&test, id).status, OutputStatus::Unspent);
    assert_eq!(balance(&test, &account), Amount::from(VALUE));
}

/// The reported version went down, but a re-versioning took it down, and the output is live at the next version.
#[tokio::test(flavor = "multi_thread")]
async fn a_verifying_wallet_keeps_an_output_reported_burnt_that_a_read_proves_live() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&test, &account, id, &utxo, Some(3), OutputStatus::Unspent);
    serve_proven_down(&sets, &network, id);
    serve_latest(&sets, &network, id, &utxo, true);
    report_burnt(&network, id);

    let stats = scan(&test).await;
    assert_eq!(stats.num_burnt, 0);
    assert_eq!(stats.num_spent, 0);
    let output = stored(&test, id);
    assert_eq!(output.status, OutputStatus::Unspent);
    assert_eq!(output.proven_up.map(|proven_up| proven_up.version), Some(BURNT_VERSION));
}

/// The check of a held output in a shard the indexer flags reads the output's latest version, which a burn leaves up
/// without an output.
#[tokio::test(flavor = "multi_thread")]
async fn a_held_output_in_a_flagged_shard_read_burnt_is_recorded_burnt() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&test, &account, id, &utxo, Some(3), OutputStatus::Unspent);
    serve_latest(&sets, &network, id, &burnt(&utxo), true);
    answer_scans(&network, id, None, true);

    let stats = scan(&test).await;
    assert_eq!(stats.num_flagged_shards, 1);
    assert_eq!(stats.num_burnt, 1);
    assert_burnt(&test, &account, id);
    assert_eq!(
        stored(&test, id).proven_up.map(|proven_up| proven_up.version),
        Some(VERSION),
        "the burnt version is not held as a live one"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_trusting_wallet_records_burnt_a_held_output_in_a_flagged_shard_served_burnt() {
    let (sets, network, test, account) = wallet(IndexerTrust::Trust).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&test, &account, id, &utxo, None, OutputStatus::LockedForSpend);
    serve_latest(&sets, &network, id, &burnt(&utxo), false);
    answer_scans(&network, id, None, true);

    let stats = scan(&test).await;
    assert_eq!(stats.num_burnt, 1);
    assert_burnt(&test, &account, id);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_held_output_without_a_proof_read_burnt_is_recorded_burnt() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&test, &account, id, &utxo, None, OutputStatus::Unspent);
    serve_latest(&sets, &network, id, &burnt(&utxo), true);
    let notify = Notify::new(100);
    let mut events = notify.subscribe();

    AccountScanner::new(notify, test.sdk().clone())
        .prove_unproven_held_outputs()
        .await
        .unwrap();
    assert_burnt(&test, &account, id);
    assert!(stored(&test, id).proven_up.is_none());
    assert_eq!(count_burnt_events(&mut events), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reported_output_held_without_a_proof_read_burnt_is_recorded_burnt() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&test, &account, id, &utxo, None, OutputStatus::Unspent);
    serve_latest(&sets, &network, id, &burnt(&utxo), true);

    recover_reported(&test, &network, &account, id, &utxo).await;
    assert_burnt(&test, &account, id);
}

/// A report that a held unfrozen output is frozen is checked against a read of the output, which a burn leaves without
/// an output.
#[tokio::test(flavor = "multi_thread")]
async fn a_held_output_reported_frozen_and_read_burnt_is_recorded_burnt() {
    let (sets, network, test, account) = wallet(IndexerTrust::Verify).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&test, &account, id, &utxo, Some(3), OutputStatus::Unspent);
    serve_latest(&sets, &network, id, &burnt(&utxo), true);
    let mut frozen = utxo.clone();
    frozen.freeze();

    recover_reported(&test, &network, &account, id, &frozen).await;
    assert_burnt(&test, &account, id);
    assert_eq!(
        stored(&test, id).proven_up.map(|proven_up| proven_up.version),
        Some(VERSION),
        "the burnt version is not held as a live one"
    );
}

/// The wallet's own transaction burnt an output it holds, so the transaction's diff brings up the burnt UTXO.
#[tokio::test(flavor = "multi_thread")]
async fn a_transaction_diff_with_the_burnt_utxo_of_a_held_output_records_it_burnt() {
    let (_, _, test, account) = wallet(IndexerTrust::Trust).await;
    let (id, utxo) = test.owned_utxo(&account, VALUE);
    hold(&test, &account, id, &utxo, None, OutputStatus::Unspent);
    let accounts_api = test.sdk().accounts_api();
    accounts_api
        .update_vault_balance_and_record_change(
            Test::test_vault_address(),
            VERSION,
            Amount::zero(),
            Amount::from(VALUE),
            BalanceChangeSource::Scan,
        )
        .unwrap();
    let mut diff = SubstateDiff::new();
    diff.up(
        substate_id(id),
        Substate::new(BURNT_VERSION, SubstateValue::Utxo(burnt(&utxo))),
    );
    let notify = Notify::new(100);
    let mut events = notify.subscribe();
    let transaction_id = TransactionId::default();

    AccountScanner::new(notify, test.sdk().clone())
        .process_result(transaction_id, &diff, None)
        .await
        .unwrap();
    assert_burnt(&test, &account, id);
    assert_eq!(count_burnt_events(&mut events), 1);
    assert_eq!(
        accounts_api
            .get_vault_balance(&Test::test_vault_address())
            .unwrap()
            .confidential,
        Amount::zero()
    );
    let changes = accounts_api
        .get_balance_changes(&Test::test_account_address(), 0, 10, None, Some(&transaction_id), None)
        .unwrap();
    assert_eq!(
        changes.total, 1,
        "the burn is recorded as the transaction's balance change"
    );
}
