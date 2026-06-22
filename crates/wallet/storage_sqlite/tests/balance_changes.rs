//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use tari_ootle_common_types::Epoch;
use tari_ootle_transaction::TransactionId;
use tari_ootle_wallet_sdk::{
    models::{
        BalanceChangeSource,
        BalanceChangeSourceType,
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
        .balance_changes_get_by_account(&account_address, 0, 10, None, None, None, None, None)
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
        .balance_changes_get_by_account(&account_address, 0, 10, None, None, None, None, None)
        .unwrap();
    assert_eq!(changes.len(), 2);

    let changes_by_tx = tx
        .balance_changes_get_by_account(&account_address, 0, 10, None, Some(tx_id), None, None, None)
        .unwrap();
    assert_eq!(changes_by_tx.len(), 2);

    let changes_by_resource = tx
        .balance_changes_get_by_account(&account_address, 0, 10, Some(&resource_address_1), None, None, None, None)
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
        .balance_changes_get_by_account(&account_address, 0, 10, None, None, None, None, None)
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

    let mut tx = db.create_write_tx().unwrap();
    tx.balance_changes_insert(
        &vault_id,
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
        .balance_changes_get_by_account(&account_address, 0, 10, None, None, None, None, None)
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

    let page_1 = tx
        .balance_changes_get_by_account(&account_address, 0, 2, None, None, None, None, None)
        .unwrap();
    assert_eq!(page_1.len(), 2);

    let page_2 = tx
        .balance_changes_get_by_account(&account_address, 2, 2, None, None, None, None, None)
        .unwrap();
    assert_eq!(page_2.len(), 2);

    let total = tx
        .balance_changes_count_by_account(&account_address, None, None, None, None, None)
        .unwrap();
    assert_eq!(total, 5);

    let filtered_total = tx
        .balance_changes_count_by_account(&account_address, Some(&resource_address), None, None, None, None)
        .unwrap();
    assert_eq!(filtered_total, 5);
}

#[test]
fn zero_balance_change_is_logged() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let (account_address, vault_id, resource_address) = setup_account_and_vault(&db);

    let tx_id = make_tx_id(1);
    let mut tx = db.create_write_tx().unwrap();
    tx.balance_changes_insert(
        &vault_id,
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
        .balance_changes_get_by_account(&account_address, 0, 10, None, None, None, None, None)
        .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].revealed_delta, "0");
    assert_eq!(changes[0].confidential_delta, "0");
}

#[test]
fn source_type_filter() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let (account_address, vault_id, resource_address) = setup_account_and_vault(&db);

    let tx_id = make_tx_id(1);
    let mut tx = db.create_write_tx().unwrap();
    tx.balance_changes_insert(
        &vault_id,
        &resource_address,
        &Amount::from(0u64),
        &Amount::from(100u64),
        &Amount::from(0u64),
        &Amount::from(0u64),
        &BalanceChangeSource::Transaction { transaction_id: tx_id },
    )
    .unwrap();
    tx.balance_changes_insert(
        &vault_id,
        &resource_address,
        &Amount::from(100u64),
        &Amount::from(200u64),
        &Amount::from(0u64),
        &Amount::from(0u64),
        &BalanceChangeSource::Recovery,
    )
    .unwrap();
    tx.balance_changes_insert(
        &vault_id,
        &resource_address,
        &Amount::from(200u64),
        &Amount::from(300u64),
        &Amount::from(0u64),
        &Amount::from(0u64),
        &BalanceChangeSource::Scan,
    )
    .unwrap();
    tx.commit().unwrap();

    let tx_changes = tx
        .balance_changes_get_by_account(&account_address, 0, 10, None, None, None, None, None)
        .unwrap();
    assert_eq!(tx_changes.len(), 3);

    let tx_filtered = tx
        .balance_changes_get_by_account(
            &account_address,
            0,
            10,
            None,
            None,
            Some(BalanceChangeSourceType::Transaction),
            None,
            None,
        )
        .unwrap();
    assert_eq!(tx_filtered.len(), 1);
    assert!(matches!(tx_filtered[0].source, BalanceChangeSource::Transaction { .. }));

    let scan_filtered = tx
        .balance_changes_get_by_account(
            &account_address,
            0,
            10,
            None,
            None,
            Some(BalanceChangeSourceType::Scan),
            None,
            None,
        )
        .unwrap();
    assert_eq!(scan_filtered.len(), 1);
    assert_eq!(scan_filtered[0].source, BalanceChangeSource::Scan);

    let count = tx
        .balance_changes_count_by_account(
            &account_address,
            None,
            None,
            Some(BalanceChangeSourceType::Recovery),
            None,
            None,
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn time_range_filter() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let (account_address, vault_id, resource_address) = setup_account_and_vault(&db);

    let tx_id_1 = make_tx_id(1);
    let tx_id_2 = make_tx_id(2);
    let mut tx = db.create_write_tx().unwrap();
    tx.balance_changes_insert(
        &vault_id,
        &resource_address,
        &Amount::from(0u64),
        &Amount::from(100u64),
        &Amount::from(0u64),
        &Amount::from(0u64),
        &BalanceChangeSource::Transaction { transaction_id: tx_id_1 },
    )
    .unwrap();
    // Small delay to ensure different created_at timestamps
    std::thread::sleep(std::time::Duration::from_millis(10));
    tx.balance_changes_insert(
        &vault_id,
        &resource_address,
        &Amount::from(100u64),
        &Amount::from(200u64),
        &Amount::from(0u64),
        &Amount::from(0u64),
        &BalanceChangeSource::Transaction { transaction_id: tx_id_2 },
    )
    .unwrap();
    tx.commit().unwrap();

    // Get one change and use its created_at as a filter boundary
    let all_changes = tx
        .balance_changes_get_by_account(&account_address, 0, 10, None, None, None, None, None)
        .unwrap();
    assert_eq!(all_changes.len(), 2);

    // Filter with start_time set to a time before both entries should return both
    let early = "2020-01-01T00:00:00Z";
    let filtered = tx
        .balance_changes_get_by_account(
            &account_address,
            0,
            10,
            None,
            None,
            None,
            Some(early),
            None,
        )
        .unwrap();
    assert_eq!(filtered.len(), 2, "Both entries are after 2020");

    // Filter with end_time before a known time should give 0 results
    let before_earliest = tx
        .balance_changes_get_by_account(
            &account_address,
            0,
            10,
            None,
            None,
            None,
            None,
            Some(early),
        )
        .unwrap();
    assert_eq!(before_earliest.len(), 0, "No changes before 2020");
}

#[test]
fn promote_scan_to_transaction() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let (account_address, vault_id, resource_address) = setup_account_and_vault(&db);

    // Insert a Scan entry with after_balance = 500 (simulating scanner seeing a tx's effect)
    let mut tx = db.create_write_tx().unwrap();
    tx.balance_changes_insert(
        &vault_id,
        &resource_address,
        &Amount::from(100u64),
        &Amount::from(500u64),
        &Amount::from(0u64),
        &Amount::from(0u64),
        &BalanceChangeSource::Scan,
    )
    .unwrap();
    tx.commit().unwrap();

    let pre_promote = tx
        .balance_changes_get_by_account(&account_address, 0, 10, None, None, None, None, None)
        .unwrap();
    assert_eq!(pre_promote.len(), 1);
    assert_eq!(pre_promote[0].source, BalanceChangeSource::Scan);

    // Promote the scan to a transaction
    let tx_id = make_tx_id(42);
    let mut promote_tx = db.create_write_tx().unwrap();
    promote_tx
        .balance_changes_promote_scan_to_transaction(
            &vault_id,
            &tx_id,
            &Amount::from(500u64),
            &Amount::from(0u64),
        )
        .unwrap();
    promote_tx.commit().unwrap();

    let post_promote = tx
        .balance_changes_get_by_account(&account_address, 0, 10, None, None, None, None, None)
        .unwrap();
    assert_eq!(post_promote.len(), 1);
    match &post_promote[0].source {
        BalanceChangeSource::Transaction { transaction_id } => {
            assert_eq!(*transaction_id, tx_id);
        },
        _ => panic!("Expected Transaction source after promotion"),
    }
    assert_eq!(
        post_promote[0].transaction_id,
        Some(tx_id.to_string())
    );

    // Verify that promoting again with same balances is a no-op
    let mut dup_promote_tx = db.create_write_tx().unwrap();
    dup_promote_tx
        .balance_changes_promote_scan_to_transaction(
            &vault_id,
            &tx_id,
            &Amount::from(500u64),
            &Amount::from(0u64),
        )
        .unwrap();
    dup_promote_tx.commit().unwrap();

    let after_dup = tx
        .balance_changes_get_by_account(&account_address, 0, 10, None, None, None, None, None)
        .unwrap();
    assert_eq!(after_dup.len(), 1, "No duplicate row created on re-promotion");
}
