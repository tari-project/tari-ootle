//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_ootle_transaction::TransactionId;
use tari_template_lib::types::{Amount, ResourceAddress, VaultId};
use time::PrimitiveDateTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceChange {
    pub vault_address: VaultId,
    pub resource_address: String,
    pub before_revealed_balance: String,
    pub after_revealed_balance: String,
    pub before_confidential_balance: String,
    pub after_confidential_balance: String,
    pub revealed_delta: String,
    pub confidential_delta: String,
    pub source: BalanceChangeSource,
    pub transaction_id: Option<String>,
    pub created_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
#[serde(tag = "type")]
pub enum BalanceChangeSource {
    Transaction { transaction_id: TransactionId },
    Scan,
    Recovery,
}

/// Lightweight source type enum for filtering — no data payload.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub enum BalanceChangeSourceType {
    Transaction,
    Scan,
    Recovery,
}

impl BalanceChangeSource {
    pub fn source_type(&self) -> BalanceChangeSourceType {
        match self {
            BalanceChangeSource::Transaction { .. } => BalanceChangeSourceType::Transaction,
            BalanceChangeSource::Scan => BalanceChangeSourceType::Scan,
            BalanceChangeSource::Recovery => BalanceChangeSourceType::Recovery,
        }
    }

    pub fn as_key_str(&self) -> &'static str {
        match self {
            BalanceChangeSource::Transaction { .. } => "Transaction",
            BalanceChangeSource::Scan => "Scan",
            BalanceChangeSource::Recovery => "Recovery",
        }
    }
}

impl BalanceChangeSourceType {
    pub fn as_key_str(&self) -> &'static str {
        match self {
            BalanceChangeSourceType::Transaction => "Transaction",
            BalanceChangeSourceType::Scan => "Scan",
            BalanceChangeSourceType::Recovery => "Recovery",
        }
    }
}

impl BalanceChange {
    pub fn compute_delta(before: Amount, after: Amount) -> String {
        let before_u128 = before.to_u128();
        let after_u128 = after.to_u128();
        if after_u128 >= before_u128 {
            (after_u128 - before_u128).to_string()
        } else {
            format!("-{}", before_u128 - after_u128)
        }
    }

    pub fn new(
        vault_address: VaultId,
        resource_address: ResourceAddress,
        before_revealed_balance: Amount,
        after_revealed_balance: Amount,
        before_confidential_balance: Amount,
        after_confidential_balance: Amount,
        source: BalanceChangeSource,
    ) -> Self {
        let revealed_delta = Self::compute_delta(before_revealed_balance, after_revealed_balance);
        let confidential_delta = Self::compute_delta(before_confidential_balance, after_confidential_balance);
        let transaction_id = match &source {
            BalanceChangeSource::Transaction { transaction_id } => Some(transaction_id.to_string()),
            BalanceChangeSource::Scan | BalanceChangeSource::Recovery => None,
        };
        Self {
            vault_address,
            resource_address: resource_address.to_string(),
            before_revealed_balance: before_revealed_balance.to_string(),
            after_revealed_balance: after_revealed_balance.to_string(),
            before_confidential_balance: before_confidential_balance.to_string(),
            after_confidential_balance: after_confidential_balance.to_string(),
            revealed_delta,
            confidential_delta,
            source,
            transaction_id,
            created_at: PrimitiveDateTime::new(time::Date::MIN, time::Time::MIDNIGHT),
        }
    }
}
