//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use tari_ootle_common_types::Epoch;
use tari_ootle_transaction::TransactionId;
use tari_ootle_wallet_sdk::{
    models::{
        BalanceChangeSource,
        KeyBranch,
        KeyId,
        VaultModel,
    },
    storage::{CommittableStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore},
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_template_lib_types::{
    Amount,
    ComponentAddress,
    ResourceAddress,
    ResourceType,
    VaultId,
    crypto::RistrettoPublicKeyBytes,
};

fn make_tx_id(val: u8) -> TransactionId {
    let mut bytes = [0u8; 32];
    bytes[31] = val;
    TransactionId::new(bytes)
}

fn setup_account_and_vault(db: &SqliteWalletStore) -> (ComponentAddress, VaultId, ResourceAddress) {
    let account_address = ComponentAddress::from_str(
        "component_91bef6af37bfb39b20260275c37a9e8acfc0517127284cd8f05944c8ffffffff",
    )
    .unwrap();
    let vault_id = VaultId::from_str(
        "vault_0000000000000000000000000000000000000000000000000000000000000001",
    )
    .unwrap();
    let resource_address = ResourceAddress::from_str(
        "resource_0000000000000000000000000000000000000000000000000000000000000001",
    )
    .unwrap();

    let mut tx = db.create_write_tx().unwrap();
    tx.accounts_insert(
        Some("test"),
        &account_address,
        KeyId::derived(KeyBranch::Account, 0),
        Some(KeyId::derived(KeyBranch::Account, 0)),
        &RistrettoPublicKeyBytes::default(),
        &Default::default(),
        Epoch::zero(),
        false,
        false,
    )
    .unwrap();
    tx.vaults_insert(VaultModel {
        account_address,
        id: vault_id,
        resource_address,
        resource_type: ResourceType::Fungible,
        confidential_balance: Amount::zero(),
        revealed_balance: Amount::zero(),
        locked_revealed_balance: Amount::zero(),
        token_symbol: Some("TARI".to_string()),
        divisibility: 6,
    })
    .unwrap();
    tx.commit().unwrap();
    (account_address, vault_id, resource_address)
}

#[test]
fn tx_driven_change() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let (account_address, vault_id, resource_address) = setup_account_and_vault(&db);

    let tx_id = make_tx_id(42);
    let mut tx = db.create_write_tx().unwrap();
    tx.balance_changes_insert(
        &vault_id,
        &account_address,
        &resource_address,
        &Amount::from(0u64),
        &Amount::from(1000u64),
        &Amount::from(0u64),
        &Amount::from(500u64),
        &BalanceChangeSource::Transaction { transaction_id: tx_id },
    )
    .unwrap();
    tx.commit().unwrap();

    let changes = tx
        .balance_changes_get_by_account(&account_address, 0, 10, None, None)
        .unwrap();
    assert_eq!(changes.len(), 1);

    let change = &changes[0];
    assert_eq!(change.vault_address, vault_id);
    assert_eq!(change.resource_address, resource_address.to_string());
    assert_eq!(change.before_revealed_balance, "0");
    assert_eq!(change.after_revealed_balance, "1000");
    assert_eq!(change.revealed_delta, "1000");
    assert_eq!(change.confidential_delta, "500");
    assert_eq!(change.transaction_id, Some(tx_id.to_string()));
    match &change.source {
        BalanceChangeSource::Transaction { transaction_id: tid } => {
            assert_eq!(*tid, tx_id);
        },
        _ => panic!("Expected Transaction source"),
    }
}

#[test]
fn multi_vault_transaction() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let (account_address, vault_id_1, resource_address_1) = setup_account_and_vault(&db);

    let vault_id_2 = VaultId::from_str(
        "vault_0000000000000000000000000000000000000000000000000000000000000002",
    )
    .unwrap();
    let resource_address_2 = ResourceAddress::from_str(
        "resource_0000000000000000000000000000000000000000000000000000000000000002",
    )
    .unwrap();

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.vaults_insert(VaultModel {
            account_address,
            id: vault_id_2,
            resource_address: resource_address_2,
            resource_type: ResourceType::Fungible,
            confidential_balance: Amount::zero(),
            revealed_balance: Amount::zero(),
            locked_revealed_balance: Amount::zero(),
            token_symbol: None,
            divisibility: 0,
        })
        .unwrap();
        tx.commit().unwrap();
    }

    let tx_id = make_tx_id(99);
    let mut tx = db.create_write_tx().unwrap();
    tx.balance_changes_insert(
        &vault_id_1,
        &account_address,
        &resource_address_1,
        &Amount::from(0u64),
        &Amount::from(500u64),
        &Amount::from(0u64),
        &Amount::from(0u64),
        &BalanceChangeSource::Transaction { transaction_id: tx_id },
    )
    .unwrap();
    tx.balance_changes_insert(
        &vault_id_2,
        &account_address,
        &resource_address_2,
        &Amount::from(100u64),
        &Amount::from(200u64),
        &Amount::from(0u64),
        &Amount::from(0u64),
        &BalanceChangeSource::Transaction { transaction_id: tx_id },
    )
    .unwrap();
    tx.commit().unwrap();

    let changes = tx
        .balance_changes_get_by_account(&account_address, 0, 10, None, None)
        .unwrap();
    assert_eq!(changes.len(), 2);

    // Filter by transaction_id
    let changes_by_tx = tx
        .balance_changes_get_by_account(&account_address, 0, 10, None, Some(tx_id))
        .unwrap();
    assert_eq!(changes_by_tx.len(), 2);

    // Filter by resource_address
    let changes_by_resource = tx
        .balance_changes_get_by_account(&account_address, 0, 10, Some(&resource_address_1), None)
        .unwrap();
    assert_eq!(changes_by_resource.len(), 1);
}

#[test]
fn non_transaction_recovery_change() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let (account_address, vault_id, resource_address) = setup_account_and_vault(&db);

    let mut tx = db.create_write_tx().unwrap();
    tx.balance_changes_insert(
        &vault_id,
        &account_address,
        &resource_address,
        &Amount::from(0u64),
        &Amount::from(500u64),
        &Amount::from(0u64),
        &Amount::from(0u64),
        &BalanceChangeSource::Recovery,
    )
    .unwrap();
    tx.commit().unwrap();

    let changes = tx
        .balance_changes_get_by_account(&account_address, 0, 10, None, None)
        .unwrap();
    assert_eq!(changes.len(), 1);
    let change = &changes[0];
    assert!(change.transaction_id.is_none());
    match &change.source {
        BalanceChangeSource::Recovery => {},
        _ => panic!("Expected Recovery source"),
    }
}

#[test]
fn idempotent_rescan() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let (account_address, vault_id, resource_address) = setup_account_and_vault(&db);

    let tx_id = make_tx_id(7);
    let source = BalanceChangeSource::Transaction { transaction_id: tx_id };

    // Insert the same change twice
    let mut tx = db.create_write_tx().unwrap();
    tx.balance_changes_insert(
        &vault_id,
        &account_address,
        &resource_address,
        &Amount::from(0u64),
        &Amount::from(100u64),
        &Amount::from(0u64),
        &Amount::from(0u64),
        &source,
    )
    .unwrap();
    tx.balance_changes_insert(
        &vault_id,
        &account_address,
        &resource_address,
        &Amount::from(0u64),
        &Amount::from(100u64),
        &Amount::from(0u64),
        &Amount::from(0u64),
        &source,
    )
    .unwrap();
    tx.commit().unwrap();

    let changes = tx
        .balance_changes_get_by_account(&account_address, 0, 10, None, None)
        .unwrap();
    assert_eq!(changes.len(), 1, "Idempotent insert should not create duplicates");
}

#[test]
fn pagination() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let (account_address, vault_id, resource_address) = setup_account_and_vault(&db);

    let mut tx = db.create_write_tx().unwrap();
    for i in 1..=5u64 {
        tx.balance_changes_insert(
            &vault_id,
            &account_address,
            &resource_address,
            &Amount::from((i - 1) * 100),
            &Amount::from(i * 100),
            &Amount::from(0u64),
            &Amount::from(0u64),
            &BalanceChangeSource::Transaction {
                transaction_id: make_tx_id(i as u8),
            },
        )
        .unwrap();
    }
    tx.commit().unwrap();

    // Fetch first 2 (newest first)
    let page_1 = tx
        .balance_changes_get_by_account(&account_address, 0, 2, None, None)
        .unwrap();
    assert_eq!(page_1.len(), 2);

    // Use offset to get next page
    let page_2 = tx
        .balance_changes_get_by_account(&account_address, 2, 2, None, None)
        .unwrap();
    assert_eq!(page_2.len(), 2);

    // Verify total count
    let total = tx
        .balance_changes_count_by_account(&account_address, None, None)
        .unwrap();
    assert_eq!(total, 5);

    // Verify filtered count
    let filtered_total = tx
        .balance_changes_count_by_account(&account_address, Some(&resource_address), None)
        .unwrap();
    assert_eq!(filtered_total, 5);
}

#[test]
fn zero_balance_change_is_logged_by_storage() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let (account_address, vault_id, resource_address) = setup_account_and_vault(&db);

    let tx_id = make_tx_id(1);
    let mut tx = db.create_write_tx().unwrap();
    tx.balance_changes_insert(
        &vault_id,
        &account_address,
        &resource_address,
        &Amount::from(100u64),
        &Amount::from(100u64),
        &Amount::from(50u64),
        &Amount::from(50u64),
        &BalanceChangeSource::Transaction { transaction_id: tx_id },
    )
    .unwrap();
    tx.commit().unwrap();

    let changes = tx
        .balance_changes_get_by_account(&account_address, 0, 10, None, None)
        .unwrap();
    // This is logged because the storage layer doesn't filter zero deltas;
    // the filtering happens in the scanner layer before calling record_balance_change.
    // The test documents the current behavior: if all balances are identical, 
    // deltas are "0" and the entry is still stored.
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].revealed_delta, "0");
    assert_eq!(changes[0].confidential_delta, "0");
}
