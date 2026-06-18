//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, str::FromStr};

use ootle_byte_type::ToByteType;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_ootle_common_types::Epoch;
use tari_ootle_transaction::{Transaction, args};
use tari_ootle_wallet_sdk::{
    models::{BalanceChangeSource, KeyBranch, KeyId, VaultModel},
    storage::{CommittableStore, ReadableWalletStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore},
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_template_lib_types::{Amount, ComponentAddress, ResourceAddress, ResourceType, VaultId};

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
    let store = SqliteWalletStore::try_open(":memory:").unwrap();
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
        tx.vaults_insert(VaultModel {
            account_address,
            id,
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

fn record_change(
    store: &SqliteWalletStore,
    vault_address: VaultId,
    revealed_after: Amount,
    confidential_after: Amount,
    source: BalanceChangeSource,
) {
    let mut tx = store.create_write_tx().unwrap();
    let current = tx.vaults_get(&vault_address).unwrap();
    tx.vaults_update(vault_address, revealed_after, confidential_after)
        .unwrap();
    assert!(
        tx.balance_changes_insert(&current, revealed_after, confidential_after, source)
            .unwrap()
    );
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
        Amount::from(100u64),
        Amount::from(7u64),
        BalanceChangeSource::Transaction { transaction_id },
    );
    record_change(
        &store,
        first_vault,
        Amount::from(40u64),
        Amount::from(2u64),
        BalanceChangeSource::Scan,
    );
    record_change(
        &store,
        second_vault,
        Amount::from(1u64),
        Amount::zero(),
        BalanceChangeSource::Recovery,
    );

    let mut tx = store.create_read_tx().unwrap();
    let changes = tx
        .balance_changes_get_by_account(&account_address(), 0, 10, None, None)
        .unwrap();
    assert_eq!(changes.len(), 3);
    assert!(changes.windows(2).all(|pair| pair[0].id > pair[1].id));

    let nft_change = changes
        .iter()
        .find(|change| change.vault_address == second_vault)
        .unwrap();
    assert_eq!(nft_change.resource_address, second_resource);
    assert_eq!(nft_change.token_symbol.as_deref(), Some("NFT"));
    assert_eq!(nft_change.divisibility, 0);
    assert_eq!(nft_change.revealed_delta, "1");
    assert_eq!(nft_change.source, BalanceChangeSource::Recovery);

    let decrease = changes
        .iter()
        .find(|change| change.vault_address == first_vault && change.source == BalanceChangeSource::Scan)
        .unwrap();
    assert_eq!(decrease.revealed_delta, "-60");
    assert_eq!(decrease.confidential_delta, "-5");
    assert_eq!(decrease.revealed_after, Amount::from(40u64));

    let by_resource = tx
        .balance_changes_get_by_account(&account_address(), 0, 10, Some(&first_resource), None)
        .unwrap();
    assert_eq!(by_resource.len(), 2);
    assert_eq!(
        tx.balance_changes_count_by_account(&account_address(), Some(&first_resource), None)
            .unwrap(),
        2
    );

    let by_transaction = tx
        .balance_changes_get_by_account(&account_address(), 0, 10, None, Some(&transaction_id))
        .unwrap();
    assert_eq!(by_transaction.len(), 1);
    assert_eq!(by_transaction[0].revealed_delta, "100");
    assert_eq!(by_transaction[0].confidential_delta, "7");
    assert_eq!(
        tx.balance_changes_count_by_account(&account_address(), None, Some(&transaction_id))
            .unwrap(),
        1
    );
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
        tx.balance_changes_insert(
            &current,
            current.revealed_balance,
            current.confidential_balance,
            BalanceChangeSource::Scan,
        )
        .is_err()
    );
    tx.rollback().unwrap();
    drop(tx);

    record_change(
        &store,
        first_vault,
        Amount::from(10u64),
        Amount::zero(),
        BalanceChangeSource::Transaction { transaction_id },
    );
    let mut tx = store.create_write_tx().unwrap();
    let current = tx.vaults_get(&first_vault).unwrap();
    assert!(
        !tx.balance_changes_insert(
            &current,
            Amount::from(11u64),
            Amount::zero(),
            BalanceChangeSource::Transaction { transaction_id },
        )
        .unwrap()
    );
    tx.commit().unwrap();
    drop(tx);

    record_change(
        &store,
        first_vault,
        Amount::from(20u64),
        Amount::zero(),
        BalanceChangeSource::Scan,
    );
    record_change(
        &store,
        first_vault,
        Amount::from(30u64),
        Amount::zero(),
        BalanceChangeSource::Recovery,
    );

    let mut tx = store.create_read_tx().unwrap();
    assert_eq!(
        tx.balance_changes_count_by_account(&account_address(), None, None)
            .unwrap(),
        3
    );
    let first_page = tx
        .balance_changes_get_by_account(&account_address(), 0, 2, None, None)
        .unwrap();
    let second_page = tx
        .balance_changes_get_by_account(&account_address(), 2, 2, None, None)
        .unwrap();
    assert_eq!(first_page.len(), 2);
    assert_eq!(second_page.len(), 1);
    assert!(first_page[0].id > first_page[1].id);
    assert!(first_page[1].id > second_page[0].id);
}
