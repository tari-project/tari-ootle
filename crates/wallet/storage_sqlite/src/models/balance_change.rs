//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::{Identifiable, Queryable};
use tari_ootle_wallet_sdk::{models::BalanceChangeSource, storage::WalletStorageError};
use tari_template_lib_types::VaultId;
use time::PrimitiveDateTime;

use crate::schema::account_balance_changes;

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = account_balance_changes)]
pub struct BalanceChangeRow {
    pub id: i32,
    pub vault_id: i32,
    pub account_id: i32,
    pub resource_address: String,
    pub before_revealed_balance: String,
    pub after_revealed_balance: String,
    pub before_confidential_balance: String,
    pub after_confidential_balance: String,
    pub revealed_delta: String,
    pub confidential_delta: String,
    pub source: String,
    pub transaction_id: Option<String>,
    pub created_at: PrimitiveDateTime,
}

impl BalanceChangeRow {
    pub fn try_into_balance_change(
        self,
        vault_address: VaultId,
    ) -> Result<tari_ootle_wallet_sdk::models::BalanceChange, WalletStorageError> {
        let source = parse_balance_change_source(&self.source, self.transaction_id.as_deref())?;
        Ok(tari_ootle_wallet_sdk::models::BalanceChange {
            vault_address,
            resource_address: self.resource_address,
            before_revealed_balance: self.before_revealed_balance,
            after_revealed_balance: self.after_revealed_balance,
            before_confidential_balance: self.before_confidential_balance,
            after_confidential_balance: self.after_confidential_balance,
            revealed_delta: self.revealed_delta,
            confidential_delta: self.confidential_delta,
            source,
            transaction_id: self.transaction_id,
            created_at: self.created_at,
        })
    }
}

pub(crate) fn balance_change_source_to_string(source: &BalanceChangeSource) -> String {
    match source {
        BalanceChangeSource::Transaction { .. } => "Transaction".to_string(),
        BalanceChangeSource::Scan => "Scan".to_string(),
        BalanceChangeSource::Recovery => "Recovery".to_string(),
    }
}

pub(crate) fn parse_balance_change_source(
    source: &str,
    transaction_id: Option<&str>,
) -> Result<BalanceChangeSource, WalletStorageError> {
    match source {
        "Transaction" => {
            let tx_id = transaction_id
                .ok_or_else(|| WalletStorageError::DecodingError {
                    operation: "parse_balance_change_source",
                    item: "transaction_id",
                    details: "Transaction source requires a transaction_id".to_string(),
                })
                .and_then(|id| {
                    tari_ootle_transaction::TransactionId::from_hex(id).map_err(|e| WalletStorageError::DecodingError {
                        operation: "parse_balance_change_source",
                        item: "transaction_id",
                        details: e.to_string(),
                    })
                })?;
            Ok(BalanceChangeSource::Transaction { transaction_id: tx_id })
        },
        "Scan" => Ok(BalanceChangeSource::Scan),
        "Recovery" => Ok(BalanceChangeSource::Recovery),
        other => Err(WalletStorageError::DecodingError {
            operation: "parse_balance_change_source",
            item: "source",
            details: format!("Unknown balance change source: {}", other),
        }),
    }
}
