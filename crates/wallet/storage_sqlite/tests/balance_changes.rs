//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashSet,
    path::Path,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use diesel::{Connection, RunQueryDsl, SqliteConnection, sql_query};
use ootle_byte_type::ToByteType;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_engine_types::resource::Resource;
use tari_ootle_common_types::Epoch;
use tari_ootle_transaction::{Transaction, args};
use tari_ootle_wallet_sdk::{
    models::{BalanceChangeSnapshot, BalanceChangeSource, BalanceChangeSourceType, KeyBranch, KeyId, VaultModel},
    storage::{CommittableStore, ReadableWalletStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore},
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_template_lib_types::{
    Amount,
    ComponentAddress,
    Metadata,
    ResourceAddress,
    ResourceType,
    SubstateOwnerRule,
    VaultId,
    access_rules::ResourceAccessRules,
    constants::TOKEN_SYMBOL,
};

fn build_transaction(seed: u64) -> Transaction {
    Transaction::builder_localnet()
        .allocate_component_address("component")
        .put_last_instruction_output_on_workspace("bucket")
        .call_method("component", "new", args!["bucket"])
        .build_and_seal(&RistrettoSecretKey::from(seed))
}

fn account_address() -> ComponentAddress {
    ComponentAddress::from_str("component_91bef6af37bfb39b20260275c37a9e8acfc0517127284cd8f05944c8ffffffff").unwrap()
}

fn vault_address(seed: u8) -> VaultId {
    format!("vault_{seed:064x}").parse().unwrap()
}

fn resource_address(seed: u8) -> ResourceAddress {
    format!("resource_{seed:064x}").parse().unwrap()
}

fn setup_store() -> (SqliteWalletStore, VaultId, VaultId, ResourceAddress, ResourceAddress) {
    setup_store_at(":memory:")
}

fn setup_store_at(path: impl AsRef<Path>) -> (SqliteWalletStore, VaultId, VaultId, ResourceAddress, ResourceAddress) {
    let store = SqliteWalletStore::try_open(path).unwrap();
    store.run_migrations().unwrap();

    let account_address = account_address();
    let first_vault = vault_address(1);
    let second_vault = vault_address(2);
    let first_resource = resource_address(1);
    let second_resource = resource_address(2);
    let owner_public_key = RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(1000)).to_byte_type();
    let mut tx = store.create_write_tx().unwrap();
    tx.accounts_insert(
        Some("test"),
        &account_address,
        KeyId::derived(KeyBranch::ViewOnlyKey, 0),
        Some(KeyId::derived(KeyBranch::Account, 0)),
        &owner_public_key,
        &HashSet::new(),
        Epoch::zero(),
        true,
        true,
    )
    .unwrap();
    for (id, resource_address, resource_type, token_symbol, divisibility) in [
        (
            first_vault,
            first_resource,
            ResourceType::Fungible,
            Some("COIN".to_string()),
            6,
        ),
        (
            second_vault,
            second_resource,
            ResourceType::NonFungible,
            Some("NFT".to_string()),
            0,
        ),
    ] {
        tx.resources_upsert(
            &resource_address,
            &Resource::new(
                resource_type,
                SubstateOwnerRule::None,
                ResourceAccessRules::new(),
                Metadata::from([(TOKEN_SYMBOL, token_symbol.clone().unwrap_or_default())]),
                None,
                None,
                divisibility,
                false,
            ),
        )
        .unwrap();
        tx.vaults_insert(VaultModel {
            account_address,
            id,
            vault_version: 0,
            resource_address,
            resource_type,
            confidential_balance: Amount::zero(),
            revealed_balance: Amount::zero(),
            locked_revealed_balance: Amount::zero(),
            token_symbol,
            divisibility,
        })
        .unwrap();
    }
    tx.commit().unwrap();
    drop(tx);

    (store, first_vault, second_vault, first_resource, second_resource)
}

fn balance_change_snapshot(
    current: &VaultModel,
    vault_version: u32,
    revealed_after: Amount,
    confidential_after: Amount,
) -> BalanceChangeSnapshot {
    BalanceChangeSnapshot {
        account_address: current.account_address,
        vault_address: Some(current.id),
        vault_version: Some(vault_version),
        resource_address: current.resource_address,
        token_symbol: current.token_symbol.clone(),
        divisibility: current.divisibility,
        revealed_before: current.revealed_balance,
        revealed_after,
        confidential_before: current.confidential_balance,
        confidential_after,
    }
}

fn temporary_database_path() -> std::path::PathBuf {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!(
        "tari-ootle-balance-changes-{}-{unique}.sqlite",
        std::process::id()
    ))
}

fn record_change(
    store: &SqliteWalletStore,
    vault_address: VaultId,
    vault_version: u32,
    revealed_after: Amount,
    confidential_after: Amount,
    source: BalanceChangeSource,
) {
    let mut tx = store.create_write_tx().unwrap();
    let current = tx.vaults_get(&vault_address).unwrap();
    assert!(
        tx.balance_changes_insert(
            balance_change_snapshot(&current, vault_version, revealed_after, confidential_after),
            source,
        )
        .unwrap()
    );
    tx.vaults_update(vault_address, vault_version, revealed_after, confidential_after)
        .unwrap();
    tx.commit().unwrap();
}

#[test]
fn records_signed_deltas_metadata_and_filters() {
    let (store, first_vault, second_vault, first_resource, second_resource) = setup_store();
    let transaction = build_transaction(1);
    let transaction_id = transaction.calculate_id();
    let mut tx = store.create_write_tx().unwrap();
    tx.transactions_insert(&transaction, None, &[account_address()], false)
        .unwrap();
    tx.commit().unwrap();
    drop(tx);

    record_change(
        &store,
        first_vault,
        1,
        Amount::from(100u64),
        Amount::from(7u64),
        BalanceChangeSource::Transaction { transaction_id },
    );
    record_change(
        &store,
        first_vault,
        2,
        Amount::from(40u64),
        Amount::from(2u64),
        BalanceChangeSource::Scan,
    );
    record_change(
        &store,
        second_vault,
        1,
        Amount::from(1u64),
        Amount::zero(),
        BalanceChangeSource::Recovery,
    );

    let mut tx = store.create_read_tx().unwrap();
    let page = tx
        .balance_changes_get_page_by_account(&account_address(), 0, 10, None, None, None)
        .unwrap();
    assert_eq!(page.total, 3);
    let changes = page.changes;
    assert_eq!(changes.len(), 3);
    assert!(changes.windows(2).all(|pair| pair[0].id > pair[1].id));

    let nft_change = changes
        .iter()
        .find(|change| change.vault_address == Some(second_vault))
        .unwrap();
    assert_eq!(nft_change.resource_address, second_resource);
    assert_eq!(nft_change.token_symbol.as_deref(), Some("NFT"));
    assert_eq!(nft_change.divisibility, 0);
    assert_eq!(nft_change.revealed_delta, "1");
    assert_eq!(nft_change.source, BalanceChangeSource::Recovery);

    let decrease = changes
        .iter()
        .find(|change| change.vault_address == Some(first_vault) && change.source == BalanceChangeSource::Scan)
        .unwrap();
    assert_eq!(decrease.revealed_delta, "-60");
    assert_eq!(decrease.confidential_delta, "-5");
    assert_eq!(decrease.revealed_after, Amount::from(40u64));

    let by_resource = tx
        .balance_changes_get_page_by_account(&account_address(), 0, 10, Some(&first_resource), None, None)
        .unwrap();
    assert_eq!(by_resource.changes.len(), 2);
    assert_eq!(by_resource.total, 2);

    let by_transaction = tx
        .balance_changes_get_page_by_account(&account_address(), 0, 10, None, Some(&transaction_id), None)
        .unwrap();
    assert_eq!(by_transaction.total, 1);
    assert_eq!(by_transaction.changes[0].revealed_delta, "100");
    assert_eq!(by_transaction.changes[0].confidential_delta, "7");
}

#[test]
fn rejects_zero_changes_deduplicates_transactions_and_paginates_deterministically() {
    let (store, first_vault, _, _, _) = setup_store();
    let transaction = build_transaction(2);
    let transaction_id = transaction.calculate_id();
    let mut tx = store.create_write_tx().unwrap();
    tx.transactions_insert(&transaction, None, &[account_address()], false)
        .unwrap();
    tx.commit().unwrap();
    drop(tx);

    let mut tx = store.create_write_tx().unwrap();
    let current = tx.vaults_get(&first_vault).unwrap();
    assert!(
        !tx.balance_changes_insert(
            balance_change_snapshot(&current, 1, current.revealed_balance, current.confidential_balance,),
            BalanceChangeSource::Scan,
        )
        .unwrap()
    );
    tx.rollback().unwrap();
    drop(tx);

    record_change(
        &store,
        first_vault,
        1,
        Amount::from(10u64),
        Amount::zero(),
        BalanceChangeSource::Transaction { transaction_id },
    );
    let mut tx = store.create_write_tx().unwrap();
    let current = tx.vaults_get(&first_vault).unwrap();
    assert!(
        !tx.balance_changes_insert(
            balance_change_snapshot(&current, 2, Amount::from(11u64), Amount::zero()),
            BalanceChangeSource::Transaction { transaction_id },
        )
        .unwrap()
    );
    tx.commit().unwrap();
    drop(tx);

    record_change(
        &store,
        first_vault,
        2,
        Amount::from(20u64),
        Amount::zero(),
        BalanceChangeSource::Scan,
    );
    record_change(
        &store,
        first_vault,
        3,
        Amount::from(30u64),
        Amount::zero(),
        BalanceChangeSource::Recovery,
    );

    let mut tx = store.create_read_tx().unwrap();
    let first_page = tx
        .balance_changes_get_page_by_account(
            &account_address(),
            0,
            2,
            None,
            None,
            Some(BalanceChangeSourceType::Scan),
        )
        .unwrap();
    assert_eq!(first_page.total, 1);
    assert_eq!(first_page.changes.len(), 1);
    let first_page = tx
        .balance_changes_get_page_by_account(&account_address(), 0, 2, None, None, None)
        .unwrap();
    let second_page = tx
        .balance_changes_get_page_by_account(&account_address(), 2, 2, None, None, None)
        .unwrap();
    assert_eq!(first_page.total, 3);
    assert_eq!(second_page.total, 3);
    assert_eq!(first_page.changes.len(), 2);
    assert_eq!(second_page.changes.len(), 1);
    assert!(first_page.changes[0].id > first_page.changes[1].id);
    assert!(first_page.changes[1].id > second_page.changes[0].id);
}

#[test]
fn records_account_resource_change_without_vault() {
    let (store, _, _, _, _) = setup_store();
    let stealth_resource = resource_address(3);
    let mut tx = store.create_write_tx().unwrap();
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
    assert!(
        tx.balance_changes_insert(
            BalanceChangeSnapshot {
                account_address: account_address(),
                vault_address: None,
                vault_version: None,
                resource_address: stealth_resource,
                token_symbol: Some("STEALTH".to_string()),
                divisibility: 6,
                revealed_before: Amount::zero(),
                revealed_after: Amount::zero(),
                confidential_before: Amount::zero(),
                confidential_after: Amount::from(33u64),
            },
            BalanceChangeSource::Scan,
        )
        .unwrap()
    );
    tx.commit().unwrap();
    drop(tx);

    let mut tx = store.create_read_tx().unwrap();
    let page = tx
        .balance_changes_get_page_by_account(&account_address(), 0, 10, Some(&stealth_resource), None, None)
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.changes[0].vault_address, None);
    assert_eq!(page.changes[0].confidential_delta, "33");
    assert_eq!(page.changes[0].token_symbol.as_deref(), Some("STEALTH"));
}

#[test]
fn attributes_only_the_exact_vault_version_across_round_trips_and_recovery() {
    let (store, first_vault, _, _, _) = setup_store();
    let first_transaction = build_transaction(3);
    let first_transaction_id = first_transaction.calculate_id();
    let second_transaction = build_transaction(4);
    let second_transaction_id = second_transaction.calculate_id();
    let mut tx = store.create_write_tx().unwrap();
    tx.transactions_insert(&first_transaction, None, &[account_address()], false)
        .unwrap();
    tx.transactions_insert(&second_transaction, None, &[account_address()], false)
        .unwrap();
    tx.commit().unwrap();
    drop(tx);

    for (version, amount) in [(1, 10u64), (2, 20), (3, 10)] {
        record_change(
            &store,
            first_vault,
            version,
            Amount::from(amount),
            Amount::zero(),
            BalanceChangeSource::Scan,
        );
    }

    let mut tx = store.create_write_tx().unwrap();
    assert!(
        tx.balance_changes_attribute_transaction(&first_vault, 1, first_transaction_id)
            .unwrap()
    );
    assert!(
        !tx.balance_changes_attribute_transaction(&first_vault, 99, second_transaction_id)
            .unwrap()
    );
    tx.commit().unwrap();
    drop(tx);

    let mut tx = store.create_read_tx().unwrap();
    let first_transaction_page = tx
        .balance_changes_get_page_by_account(&account_address(), 0, 10, None, Some(&first_transaction_id), None)
        .unwrap();
    assert_eq!(first_transaction_page.total, 1);
    drop(tx);
    record_change(
        &store,
        first_vault,
        4,
        Amount::from(30u64),
        Amount::zero(),
        BalanceChangeSource::Recovery,
    );

    let mut tx = store.create_write_tx().unwrap();
    assert!(
        tx.balance_changes_attribute_transaction(&first_vault, 4, second_transaction_id)
            .unwrap()
    );
    assert!(
        tx.balance_changes_attribute_transaction(&first_vault, 999, second_transaction_id)
            .unwrap()
    );
    tx.commit().unwrap();
    drop(tx);

    let mut tx = store.create_read_tx().unwrap();
    let transaction_page = tx
        .balance_changes_get_page_by_account(&account_address(), 0, 10, None, Some(&second_transaction_id), None)
        .unwrap();
    assert_eq!(transaction_page.total, 1);
    assert_eq!(transaction_page.changes[0].revealed_delta, "20");
    assert_eq!(transaction_page.changes[0].source, BalanceChangeSource::Transaction {
        transaction_id: second_transaction_id,
    });
}

#[test]
fn same_version_scan_updates_after_balance_and_deletes_net_zero_row() {
    let (store, first_vault, _, _, _) = setup_store();
    record_change(
        &store,
        first_vault,
        1,
        Amount::zero(),
        Amount::from(10u64),
        BalanceChangeSource::Scan,
    );

    let mut tx = store.create_write_tx().unwrap();
    let current = tx.vaults_get(&first_vault).unwrap();
    assert!(
        tx.balance_changes_insert(
            balance_change_snapshot(&current, 1, Amount::zero(), Amount::from(15u64)),
            BalanceChangeSource::Scan,
        )
        .unwrap()
    );
    tx.vaults_update(first_vault, 1, Amount::zero(), Amount::from(15u64))
        .unwrap();
    tx.commit().unwrap();
    drop(tx);

    let mut tx = store.create_read_tx().unwrap();
    let page = tx
        .balance_changes_get_page_by_account(&account_address(), 0, 10, None, None, None)
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.changes[0].confidential_before, Amount::zero());
    assert_eq!(page.changes[0].confidential_after, Amount::from(15u64));
    assert_eq!(page.changes[0].confidential_delta, "15");
    let vault = tx.vaults_get(&first_vault).unwrap();
    assert_eq!(vault.vault_version, 1);
    assert_eq!(vault.confidential_balance, Amount::from(15u64));
    drop(tx);

    let mut tx = store.create_write_tx().unwrap();
    let current = tx.vaults_get(&first_vault).unwrap();
    assert!(
        tx.balance_changes_insert(
            balance_change_snapshot(&current, 1, Amount::zero(), Amount::zero()),
            BalanceChangeSource::Scan,
        )
        .unwrap()
    );
    tx.vaults_update(first_vault, 1, Amount::zero(), Amount::zero())
        .unwrap();
    tx.commit().unwrap();
    drop(tx);

    let mut tx = store.create_read_tx().unwrap();
    let page = tx
        .balance_changes_get_page_by_account(&account_address(), 0, 10, None, None, None)
        .unwrap();
    assert_eq!(page.total, 0);
    let vault = tx.vaults_get(&first_vault).unwrap();
    assert_eq!(vault.confidential_balance, Amount::zero());
}

#[test]
fn same_version_scan_does_not_clobber_transaction_row() {
    let (store, first_vault, _, _, _) = setup_store();
    let transaction = build_transaction(5);
    let transaction_id = transaction.calculate_id();
    let mut tx = store.create_write_tx().unwrap();
    tx.transactions_insert(&transaction, None, &[account_address()], false)
        .unwrap();
    tx.commit().unwrap();
    drop(tx);

    record_change(
        &store,
        first_vault,
        1,
        Amount::from(10u64),
        Amount::zero(),
        BalanceChangeSource::Transaction { transaction_id },
    );

    let mut tx = store.create_write_tx().unwrap();
    let current = tx.vaults_get(&first_vault).unwrap();
    assert!(
        !tx.balance_changes_insert(
            balance_change_snapshot(&current, 1, Amount::from(20u64), Amount::zero()),
            BalanceChangeSource::Scan,
        )
        .unwrap()
    );
    tx.vaults_update(first_vault, 1, Amount::from(20u64), Amount::zero())
        .unwrap();
    tx.commit().unwrap();
    drop(tx);

    let mut tx = store.create_read_tx().unwrap();
    let page = tx
        .balance_changes_get_page_by_account(&account_address(), 0, 10, None, None, None)
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.changes[0].source, BalanceChangeSource::Transaction {
        transaction_id
    });
    assert_eq!(page.changes[0].revealed_after, Amount::from(10u64));
    let vault = tx.vaults_get(&first_vault).unwrap();
    assert_eq!(vault.revealed_balance, Amount::from(20u64));
}

#[test]
fn history_keeps_snapshot_metadata_after_live_rows_are_changed_or_deleted() {
    let path = temporary_database_path();
    let (store, first_vault, _, _, _) = setup_store_at(&path);
    record_change(
        &store,
        first_vault,
        1,
        Amount::from(100u64),
        Amount::zero(),
        BalanceChangeSource::Scan,
    );
    drop(store);

    let mut connection = SqliteConnection::establish(path.to_str().unwrap()).unwrap();
    sql_query("UPDATE vaults SET token_symbol = 'CHANGED', divisibility = 9")
        .execute(&mut connection)
        .unwrap();
    drop(connection);

    let store = SqliteWalletStore::try_open(&path).unwrap();
    let mut tx = store.create_read_tx().unwrap();
    let page = tx
        .balance_changes_get_page_by_account(&account_address(), 0, 10, None, None, None)
        .unwrap();
    assert_eq!(page.changes[0].token_symbol.as_deref(), Some("COIN"));
    assert_eq!(page.changes[0].divisibility, 6);
    drop(tx);
    drop(store);

    let mut connection = SqliteConnection::establish(path.to_str().unwrap()).unwrap();
    sql_query("PRAGMA foreign_keys = ON").execute(&mut connection).unwrap();
    sql_query("DELETE FROM vaults").execute(&mut connection).unwrap();
    sql_query("DELETE FROM accounts").execute(&mut connection).unwrap();
    drop(connection);

    let store = SqliteWalletStore::try_open(&path).unwrap();
    let mut tx = store.create_read_tx().unwrap();
    let page = tx
        .balance_changes_get_page_by_account(&account_address(), 0, 10, None, None, None)
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.changes[0].vault_address, Some(first_vault));
    drop(tx);
    drop(store);
    std::fs::remove_file(path).unwrap();
}
