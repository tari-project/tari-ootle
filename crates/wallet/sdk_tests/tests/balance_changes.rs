//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod support;

use std::str::FromStr;

use tari_crypto::ristretto::RistrettoSecretKey;
use tari_ootle_common_types::Epoch;
use tari_ootle_transaction::{Transaction, TransactionId, args};
use tari_ootle_wallet_sdk::{
    models::{BalanceChangeSource, BalanceChangeSourceType, KeyBranch, KeyId},
    storage::{WalletStoreWriter, WriteableWalletStore},
};
use tari_template_lib::types::{Amount, ComponentAddress, ResourceType, VaultId, constants::TARI_TOKEN};

use crate::support::Test;

fn build_transaction() -> Transaction {
    Transaction::builder_localnet()
        .allocate_component_address("component")
        .put_last_instruction_output_on_workspace("bucket")
        .call_method("component", "new", args!["bucket"])
        .build_and_seal(&RistrettoSecretKey::from(1))
}

#[test]
fn transaction_changes_can_span_accounts_and_vaults() {
    let test = Test::new();
    let accounts = test.sdk().accounts_api();
    let second_account =
        ComponentAddress::from_str("component_91bef6af37bfb39b20260275c37a9e8acfc0517127284cd8f05944c8aaaaaaaa")
            .unwrap();
    let second_vault =
        VaultId::from_str("vault_0000000000000000000000000000000000000000000000000000000000000002").unwrap();
    accounts
        .add_account(
            Some("second"),
            &second_account,
            KeyId::derived(KeyBranch::ViewOnlyKey, 1),
            KeyId::derived(KeyBranch::Account, 1),
            Epoch::zero(),
            true,
            false,
        )
        .unwrap();
    accounts
        .add_vault(
            second_account,
            second_vault,
            TARI_TOKEN,
            ResourceType::Fungible,
            Some("XTR".to_string()),
            6,
        )
        .unwrap();

    let transaction = build_transaction();
    let transaction_id = transaction.calculate_id();
    test.store()
        .with_write_tx(|tx| {
            tx.transactions_insert(
                &transaction,
                None,
                &[Test::test_account_address(), second_account],
                false,
            )
        })
        .unwrap();

    let source = BalanceChangeSource::Transaction { transaction_id };
    assert!(
        accounts
            .update_vault_balance_and_record_change(
                Test::test_vault_address(),
                1,
                Amount::from(25u64),
                Amount::from(5u64),
                source
            )
            .unwrap()
    );
    assert!(
        accounts
            .update_vault_balance_and_record_change(second_vault, 1, Amount::from(9u64), Amount::zero(), source)
            .unwrap()
    );

    for (account, expected_delta) in [(Test::test_account_address(), "25"), (second_account, "9")] {
        let page = accounts
            .get_balance_changes(&account, 0, 10, None, Some(&transaction_id), None)
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.changes[0].revealed_delta, expected_delta);
        assert_eq!(page.changes[0].source, source);
    }
}

#[test]
fn scan_and_recovery_are_deduplicated_and_duplicate_version_cannot_update_balance() {
    let test = Test::new();
    let accounts = test.sdk().accounts_api();
    let vault = Test::test_vault_address();

    assert!(
        accounts
            .update_vault_balance_and_record_change(
                vault,
                1,
                Amount::from(10u64),
                Amount::zero(),
                BalanceChangeSource::Scan
            )
            .unwrap()
    );
    assert!(
        !accounts
            .update_vault_balance_and_record_change(
                vault,
                1,
                Amount::from(10u64),
                Amount::zero(),
                BalanceChangeSource::Scan
            )
            .unwrap()
    );
    assert!(
        accounts
            .update_vault_balance_and_record_change(
                vault,
                2,
                Amount::from(12u64),
                Amount::zero(),
                BalanceChangeSource::Recovery,
            )
            .unwrap()
    );
    let page = accounts
        .get_balance_changes(&Test::test_account_address(), 0, 10, None, None, None)
        .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.changes[0].source, BalanceChangeSource::Recovery);
    assert_eq!(page.changes[1].source, BalanceChangeSource::Scan);

    let result = accounts.update_vault_balance_and_record_change(
        vault,
        2,
        Amount::from(99u64),
        Amount::zero(),
        BalanceChangeSource::Transaction {
            transaction_id: TransactionId::default(),
        },
    );
    assert!(!result.unwrap());
    assert_eq!(
        accounts.get_vault_balance(&vault).unwrap().revealed,
        Amount::from(12u64)
    );
    assert_eq!(
        accounts
            .get_balance_changes(&Test::test_account_address(), 0, 10, None, None, None)
            .unwrap()
            .total,
        2
    );
}

#[test]
fn transaction_source_attributes_existing_scan_for_exact_vault_version() {
    let test = Test::new();
    let accounts = test.sdk().accounts_api();
    let vault = Test::test_vault_address();
    let transaction = build_transaction();
    let transaction_id = transaction.calculate_id();
    test.store()
        .with_write_tx(|tx| tx.transactions_insert(&transaction, None, &[Test::test_account_address()], false))
        .unwrap();

    assert!(
        accounts
            .update_vault_balance_and_record_change(
                vault,
                7,
                Amount::from(10u64),
                Amount::zero(),
                BalanceChangeSource::Scan,
            )
            .unwrap()
    );
    let source = BalanceChangeSource::Transaction { transaction_id };
    assert!(
        accounts
            .attribute_balance_change_to_transaction(&vault, 7, transaction_id)
            .unwrap()
    );
    assert!(
        accounts
            .attribute_balance_change_to_transaction(&vault, 99, transaction_id)
            .unwrap()
    );

    let page = accounts
        .get_balance_changes(&Test::test_account_address(), 0, 10, None, Some(&transaction_id), None)
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.changes[0].source, source);
    assert_eq!(page.changes[0].transaction_id, Some(transaction_id));
    assert_eq!(page.changes[0].revealed_delta, "10");
    assert_eq!(
        accounts
            .get_balance_changes(
                &Test::test_account_address(),
                0,
                10,
                None,
                None,
                Some(BalanceChangeSourceType::Scan),
            )
            .unwrap()
            .total,
        0
    );
}

#[test]
fn replayed_transaction_cannot_update_a_vault_without_a_new_history_row() {
    let test = Test::new();
    let accounts = test.sdk().accounts_api();
    let vault = Test::test_vault_address();
    let transaction = build_transaction();
    let transaction_id = transaction.calculate_id();
    test.store()
        .with_write_tx(|tx| tx.transactions_insert(&transaction, None, &[Test::test_account_address()], false))
        .unwrap();

    let source = BalanceChangeSource::Transaction { transaction_id };
    assert!(
        accounts
            .update_vault_balance_and_record_change(vault, 1, Amount::from(10u64), Amount::zero(), source)
            .unwrap()
    );
    assert!(
        accounts
            .update_vault_balance_and_record_change(
                vault,
                2,
                Amount::from(20u64),
                Amount::zero(),
                BalanceChangeSource::Scan,
            )
            .unwrap()
    );
    assert!(
        !accounts
            .update_vault_balance_and_record_change(vault, 1, Amount::from(15u64), Amount::zero(), source)
            .unwrap()
    );

    assert_eq!(
        accounts.get_vault_balance(&vault).unwrap().revealed,
        Amount::from(20u64)
    );
    assert_eq!(
        accounts
            .get_balance_changes(&Test::test_account_address(), 0, 10, None, None, None)
            .unwrap()
            .total,
        2
    );
}
