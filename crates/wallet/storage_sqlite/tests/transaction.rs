//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::RistrettoSecretKey;
use tari_ootle_common_types::optional::Optional;
use tari_ootle_wallet_sdk::{
    models::TransactionStatus,
    storage::{CommittableStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore},
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_transaction::{args, Transaction, TransactionId};

fn build_transaction() -> Transaction {
    let key = RistrettoSecretKey::from(123);
    Transaction::builder()
        .allocate_component_address("component")
        .put_last_instruction_output_on_workspace("bucket")
        .call_method("component", "new", args!["bucket"])
        .with_dry_run(true)
        .build_and_seal(&key)
}

#[test]
fn get_and_insert_transaction() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let mut tx = db.create_write_tx().unwrap();
    let transaction = tx.transactions_get(TransactionId::default()).optional().unwrap();
    assert!(transaction.is_none());

    let transaction = build_transaction();
    assert!(transaction.verify_all_signatures());
    let tx_id = transaction.calculate_id();
    tx.transactions_insert(&transaction, None, false).unwrap();
    tx.commit().unwrap();

    let returned = tx.transactions_get(tx_id).unwrap();
    // Transaction was not malleated in the database
    assert!(returned.transaction.verify_all_signatures());
    assert_eq!(returned.transaction.calculate_id(), tx_id);
    assert_eq!(returned.status, TransactionStatus::default());
}
