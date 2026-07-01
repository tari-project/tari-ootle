//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use diesel::{Identifiable, Queryable};
use tari_ootle_transaction::TransactionId;
use tari_ootle_wallet_sdk::{
    models::{BalanceChange, BalanceChangeSource, BalanceChangeSourceType},
    storage::WalletStorageError,
};
use time::PrimitiveDateTime;

use crate::{schema::account_balance_changes, serialization::deserialize_hex_try_from};

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = account_balance_changes)]
pub struct BalanceChangeRecord {
    pub id: i32,
    pub account_id: i32,
    pub resource_id: i32,
    pub account_address: String,
    pub vault_address: Option<String>,
    pub vault_version: Option<i64>,
    pub resource_address: String,
    pub token_symbol: Option<String>,
    pub divisibility: i32,
    pub source_type: String,
    pub transaction_id: Option<String>,
    pub revealed_before: String,
    pub revealed_after: String,
    pub confidential_before: String,
    pub confidential_after: String,
    pub created_at: PrimitiveDateTime,
}

impl BalanceChangeRecord {
    pub fn try_into_balance_change(self) -> Result<BalanceChange, WalletStorageError> {
        const OPERATION: &str = "balance_change_record_to_model";
        let transaction_id = self
            .transaction_id
            .as_deref()
            .map(deserialize_hex_try_from::<TransactionId, _>)
            .transpose()?;
        let source_type =
            BalanceChangeSourceType::from_str(&self.source_type).map_err(|e| WalletStorageError::DecodingError {
                operation: OPERATION,
                item: "source_type",
                details: e.to_string(),
            })?;
        let source = match source_type {
            BalanceChangeSourceType::Transaction => BalanceChangeSource::Transaction {
                transaction_id: transaction_id.ok_or_else(|| WalletStorageError::DataInconsistent {
                    operation: OPERATION,
                    details: "transaction source has no transaction id".to_string(),
                })?,
            },
            BalanceChangeSourceType::Scan => BalanceChangeSource::Scan,
            BalanceChangeSourceType::Recovery => BalanceChangeSource::Recovery,
        };

        let revealed_before = parse_value(OPERATION, "revealed_before", &self.revealed_before)?;
        let revealed_after = parse_value(OPERATION, "revealed_after", &self.revealed_after)?;
        let confidential_before = parse_value(OPERATION, "confidential_before", &self.confidential_before)?;
        let confidential_after = parse_value(OPERATION, "confidential_after", &self.confidential_after)?;

        Ok(BalanceChange {
            id: self.id,
            account_address: parse_value(OPERATION, "account_address", &self.account_address)?,
            vault_address: self
                .vault_address
                .as_deref()
                .map(|value| parse_value(OPERATION, "vault_address", value))
                .transpose()?,
            resource_address: parse_value(OPERATION, "resource_address", &self.resource_address)?,
            token_symbol: self.token_symbol,
            divisibility: u8::try_from(self.divisibility).map_err(|e| WalletStorageError::DecodingError {
                operation: OPERATION,
                item: "divisibility",
                details: e.to_string(),
            })?,
            revealed_before,
            revealed_after,
            confidential_before,
            confidential_after,
            revealed_delta: BalanceChange::signed_delta(revealed_before, revealed_after),
            confidential_delta: BalanceChange::signed_delta(confidential_before, confidential_after),
            source,
            transaction_id,
            created_at: self.created_at,
        })
    }
}

fn parse_value<T: FromStr>(operation: &'static str, item: &'static str, value: &str) -> Result<T, WalletStorageError>
where T::Err: std::fmt::Display {
    value.parse::<T>().map_err(|e| WalletStorageError::DecodingError {
        operation,
        item,
        details: e.to_string(),
    })
}
