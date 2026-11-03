//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Storage-level tests for lock timeout behaviour.
//!
//! A wallet lock holds selected inputs (`LockedForSpend`) and the change
//! outputs created alongside them (`LockedUnconfirmed`). The transfer-selection
//! handlers create locks with a short timeout, and the transaction service
//! periodically sweeps any lock past its deadline. A transaction request must
//! hold its inputs across a human approval window that is longer than that
//! deadline, so it extends the timeout of the locks it is given.

use std::{collections::HashSet, str::FromStr, time::Duration};

use ootle_byte_type::ToByteType;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_engine_types::{
    Utxo,
    resource::Resource,
    substate::{Substate, SubstateDiff, SubstateId},
};
use tari_ootle_common_types::{Epoch, SubstateVersion, optional::IsNotFoundError};
use tari_ootle_transaction::{Transaction, TransactionId, args};
use tari_ootle_wallet_sdk::{
    models::{
        BalanceChangeSource,
        KeyBranch,
        KeyId,
        LockOwner,
        OutputStatus,
        StealthOutputModel,
        VaultModel,
        WalletLockId,
    },
    storage::{CommittableStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore},
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_template_lib_types::{
    Amount,
    ComponentAddress,
    EncryptedData,
    Metadata,
    ResourceAddress,
    ResourceType,
    SubstateOwnerRule,
    VaultId,
    access_rules::ResourceAccessRules,
    constants::TOKEN_SYMBOL,
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, UtxoTag},
    stealth::SpendAuthorization,
};

fn open_store() -> SqliteWalletStore {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    db
}

#[test]
fn a_lock_past_its_deadline_is_swept() {
    // Control for the tests below: without it they would pass even if a zero
    // timeout were never stale in the first place.
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();

    tx.locks_create(Some(Duration::ZERO), LockOwner::Wallet).unwrap();

    assert_eq!(tx.locks_release_stale().unwrap(), 1, "a lock at its deadline is stale");
    tx.commit().unwrap();
}

#[test]
fn a_lock_linked_to_a_transaction_is_not_swept() {
    // A lock tied to an in-flight transaction is released by that
    // transaction's resolution, never by the stale sweep -- reaping it mid-flight
    // would unlock the inputs and delete the change outputs of a live
    // transaction. The link protects it regardless of its timeout.
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();

    let lock = tx.locks_create(Some(Duration::ZERO), LockOwner::Wallet).unwrap();
    tx.locks_link_transaction(lock, TransactionId::new([1u8; 32])).unwrap();

    assert_eq!(
        tx.locks_release_stale().unwrap(),
        0,
        "a lock linked to a transaction is not stale even past its deadline"
    );
    tx.commit().unwrap();
}

#[test]
fn extending_the_timeout_of_an_unknown_lock_is_not_found() {
    // `transaction_requests.create` extends caller-named locks. A bad id must
    // surface here, not silently create a request whose locks were never
    // extended past the approval window.
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();

    let err = tx.locks_extend_timeout(9999, Duration::from_secs(60)).unwrap_err();
    assert!(err.is_not_found_error(), "expected NotFound, got: {err}");
}

#[test]
fn a_linked_lock_has_no_timeout_to_extend() {
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();

    let lock = tx.locks_create(Some(Duration::ZERO), LockOwner::Wallet).unwrap();
    tx.locks_link_transaction(lock, TransactionId::new([1u8; 32])).unwrap();

    let err = tx.locks_extend_timeout(lock, Duration::from_secs(60)).unwrap_err();
    assert!(err.is_not_found_error(), "expected NotFound, got: {err}");
}

#[test]
fn extending_a_timeout_saves_a_lock_from_the_stale_sweep() {
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();

    // A zero timeout puts `timeout_at` at the current second, so the sweep
    // considers the lock stale immediately. See the companion test.
    let lock = tx.locks_create(Some(Duration::ZERO), LockOwner::Wallet).unwrap();
    tx.locks_extend_timeout(lock, Duration::from_secs(60 * 60)).unwrap();

    assert_eq!(
        tx.locks_release_stale().unwrap(),
        0,
        "a lock whose timeout was extended must survive the sweep"
    );
    tx.commit().unwrap();
}

#[test]
fn extending_never_brings_a_deadline_forward() {
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();

    let later = tx
        .locks_create(Some(Duration::from_secs(24 * 60 * 60)), LockOwner::Wallet)
        .unwrap();
    let deadline = tx.locks_get(later).unwrap().timeout_at;
    tx.locks_extend_timeout(later, Duration::ZERO).unwrap();
    assert_eq!(tx.locks_get(later).unwrap().timeout_at, deadline);

    let open = tx.locks_create(None, LockOwner::Wallet).unwrap();
    tx.locks_extend_timeout(open, Duration::ZERO).unwrap();
    assert_eq!(tx.locks_get(open).unwrap().timeout_at, None, "no deadline stays none");

    assert_eq!(tx.locks_release_stale().unwrap(), 0);
    tx.commit().unwrap();
}

#[test]
fn a_lock_records_its_owner() {
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();

    for owner in [
        LockOwner::Wallet,
        LockOwner::LocalCookie,
        LockOwner::Delegated(u64::MAX),
        LockOwner::Delegated(3),
        LockOwner::ApiKey(7),
    ] {
        let lock = tx.locks_create(None, owner).unwrap();
        assert_eq!(tx.locks_get(lock).unwrap().owner, owner);
    }
}

#[test]
fn a_transaction_holds_every_lock_linked_to_it() {
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();
    let transaction_id = TransactionId::new([1u8; 32]);

    let first = tx.locks_create(None, LockOwner::Wallet).unwrap();
    let second = tx.locks_create(None, LockOwner::Wallet).unwrap();
    tx.locks_link_transaction(first, transaction_id).unwrap();
    tx.locks_link_transaction(second, transaction_id).unwrap();

    assert_eq!(tx.locks_get_by_transaction_id(transaction_id).unwrap(), vec![
        first, second
    ]);
    assert!(
        tx.locks_get_by_transaction_id(TransactionId::new([2u8; 32]))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_lock_linked_to_one_transaction_cannot_be_linked_to_another() {
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();
    let linked_to = TransactionId::new([1u8; 32]);

    let lock = tx.locks_create(None, LockOwner::Wallet).unwrap();
    tx.locks_link_transaction(lock, linked_to).unwrap();

    let err = tx
        .locks_link_transaction(lock, TransactionId::new([2u8; 32]))
        .unwrap_err();
    assert!(err.is_not_found_error(), "expected NotFound, got: {err}");
    assert_eq!(tx.locks_get(lock).unwrap().transaction_id, Some(linked_to));

    tx.locks_link_transaction(lock, linked_to)
        .expect("linking a lock again to its own transaction is a no-op");
}

#[test]
fn linking_an_unknown_lock_is_not_found() {
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();

    let err = tx
        .locks_link_transaction(9999, TransactionId::new([1u8; 32]))
        .unwrap_err();
    assert!(err.is_not_found_error(), "expected NotFound, got: {err}");
}

#[test]
fn unlinking_clears_only_a_link_to_the_named_transaction() {
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();
    let linked_to = TransactionId::new([1u8; 32]);

    let lock = tx.locks_create(None, LockOwner::Wallet).unwrap();
    tx.locks_link_transaction(lock, linked_to).unwrap();

    tx.locks_unlink_transaction(lock, TransactionId::new([2u8; 32]))
        .unwrap();
    assert_eq!(tx.locks_get(lock).unwrap().transaction_id, Some(linked_to));

    tx.locks_unlink_transaction(lock, linked_to).unwrap();
    assert_eq!(tx.locks_get(lock).unwrap().transaction_id, None);
}

fn stealth_output(
    account: ComponentAddress,
    resource_address: ResourceAddress,
    value: u64,
    seed: u8,
    status: OutputStatus,
    lock_id: Option<WalletLockId>,
) -> StealthOutputModel {
    StealthOutputModel {
        owner_account: account,
        resource_address,
        commitment: PedersenCommitmentBytes::from_array([seed; PedersenCommitmentBytes::length()]),
        value,
        sender_public_nonce: RistrettoPublicKeyBytes::from_bytes(
            &[seed.wrapping_add(1); RistrettoPublicKeyBytes::length()],
        )
        .unwrap(),
        view_only_key_id: KeyId::derived(KeyBranch::ViewOnlyKey, 0),
        owner_key_id: Some(KeyId::derived(KeyBranch::Account, 0)),
        encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
        tag_byte: UtxoTag::new(u32::from(seed)),
        memo: None,
        auth: SpendAuthorization::Key(RistrettoPublicKeyBytes::default()),
        minimum_value_promise: 0,
        status,
        is_burnt: false,
        is_frozen: false,
        is_on_chain: !matches!(status, OutputStatus::LockedUnconfirmed),
        is_condition_spendable: true,
        lock_id,
        proven_up: None,
        spent_version: None,
    }
}

#[test]
fn finalizing_a_lock_marks_burnt_stealth_outputs_as_burnt_and_records_balance_change() {
    let db = open_store();
    let account =
        ComponentAddress::from_str("component_91bef6af37bfb39b20260275c37a9e8acfc0517127284cd8f05944c8ffffffff")
            .unwrap();
    let stealth_resource: ResourceAddress = format!("resource_{:064x}", 3u8).parse().unwrap();
    let stealth_vault: VaultId = format!("vault_{:064x}", 3u8).parse().unwrap();
    let owner_public_key = RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(1000u64)).to_byte_type();
    let transaction = Transaction::builder_localnet(Epoch(1))
        .allocate_component_address("component")
        .put_last_instruction_output_on_workspace("bucket")
        .call_method("component", "new", args!["bucket"])
        .build_and_seal(&RistrettoSecretKey::from(42u64));
    let transaction_id = transaction.calculate_id();

    let mut tx = db.create_write_tx().unwrap();
    tx.accounts_insert(
        Some("test"),
        &account,
        KeyId::derived(KeyBranch::ViewOnlyKey, 0),
        Some(KeyId::derived(KeyBranch::Account, 0)),
        &owner_public_key,
        &HashSet::new(),
        Epoch::zero(),
        true,
        true,
    )
    .unwrap();
    tx.resources_upsert(
        &stealth_resource,
        &Resource::new(
            ResourceType::Stealth,
            SubstateOwnerRule::None,
            ResourceAccessRules::new(),
            Metadata::from([(TOKEN_SYMBOL, "STEALTH".to_string())]),
            None,
            None,
            6,
            false,
        ),
    )
    .unwrap();
    tx.vaults_insert(VaultModel {
        account_address: account,
        id: stealth_vault,
        vault_version: SubstateVersion::ZERO,
        resource_address: stealth_resource,
        resource_type: ResourceType::Stealth,
        confidential_balance: Amount::from(1_000_000u64),
        revealed_balance: Amount::zero(),
        locked_revealed_balance: Amount::zero(),
        token_symbol: Some("STEALTH".to_string()),
        divisibility: 6,
    })
    .unwrap();
    tx.transactions_insert(&transaction, None, &[account], false).unwrap();

    let burnt_input = stealth_output(account, stealth_resource, 400_000, 7, OutputStatus::Unspent, None);
    let kept_output = stealth_output(account, stealth_resource, 600_000, 8, OutputStatus::Unspent, None);
    tx.stealth_outputs_insert(&burnt_input).unwrap();
    tx.stealth_outputs_insert(&kept_output).unwrap();

    let lock_id = tx.locks_create(None, LockOwner::Wallet).unwrap();
    tx.locks_link_transaction(lock_id, transaction_id).unwrap();
    tx.stealth_outputs_lock_many(&stealth_resource, &[&burnt_input.commitment], lock_id)
        .unwrap();

    let burnt_addr = burnt_input.to_utxo_address();
    let mut diff = SubstateDiff::new();
    diff.down(SubstateId::Utxo(burnt_addr.clone()), SubstateVersion::new(2));
    diff.up(
        SubstateId::Utxo(burnt_addr),
        Substate::new(SubstateVersion::new(3), Utxo {
            output: None,
            is_frozen: false,
        }),
    );

    tx.locks_unlock_finalized(lock_id, &diff).unwrap();

    let stored = tx
        .stealth_outputs_get_by_commitment(&stealth_resource, &burnt_input.commitment)
        .unwrap();
    assert_eq!(stored.status, OutputStatus::Burnt);
    assert!(stored.is_burnt);
    assert_eq!(stored.lock_id, None);
    assert_eq!(stored.spent_version, Some(SubstateVersion::new(2)));

    let unspent = tx
        .stealth_outputs_get_unspent_by_account(&account, Some(&stealth_resource), false)
        .unwrap();
    assert_eq!(unspent.len(), 1);
    assert_eq!(unspent[0].commitment, kept_output.commitment);

    let vault = tx.vaults_get(&stealth_vault).unwrap();
    assert_eq!(vault.confidential_balance, Amount::from(600_000u64));

    let page = tx
        .balance_changes_get_page_by_account(&account, 0, 10, Some(&stealth_resource), None, None)
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.changes[0].source, BalanceChangeSource::Transaction { transaction_id });
    assert_eq!(page.changes[0].confidential_before, Amount::from(1_000_000u64));
    assert_eq!(page.changes[0].confidential_after, Amount::from(600_000u64));
    assert_eq!(page.changes[0].confidential_delta, "-400000");

    tx.commit().unwrap();
}
