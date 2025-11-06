//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use tari_ootle_common_types::Epoch;
use tari_ootle_wallet_sdk::{
    models::{AccountUpdate, KeyId},
    storage::{CommittableStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore},
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_template_lib::{models::ComponentAddress, prelude::RistrettoPublicKeyBytes};

#[test]
fn update_account() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let address =
        ComponentAddress::from_str("component_91bef6af37bfb39b20260275c37a9e8acfc0517127284cd8f05944c8ffffffff")
            .unwrap();
    let mut tx = db.create_write_tx().unwrap();
    tx.accounts_insert(
        Some("test"),
        &address,
        KeyId::derived(0),
        Some(KeyId::derived(0)),
        &RistrettoPublicKeyBytes::default(),
        &Default::default(),
        Epoch::zero(),
        false,
        false,
    )
    .unwrap();
    tx.accounts_update(&address, AccountUpdate {
        name: Some("foo"),
        ..Default::default()
    })
    .unwrap();
    tx.commit().unwrap();

    let account = tx.accounts_get_by_name("foo").unwrap();
    assert_eq!(account.name.as_deref(), Some("foo"));
}
