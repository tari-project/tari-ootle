//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use chrono::NaiveDateTime;
use tari_ootle_transaction::transaction_id::TransactionId;
use tari_template_lib::models::{Amount, ComponentAddress, ResourceAddress, VaultId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BalanceChangeSource {
    Transaction { transaction_id: TransactionId },
    Scan,
    Recovery,
}
impl BalanceChangeSource {
    pub fn source_tag(&self) -> i32 {
        match self { Self::Transaction { .. } => 0, Self::Scan => 1, Self::Recovery => 2 }
    }
    pub fn transaction_id(&self) -> Option<&TransactionId> {
        if let Self::Transaction { transaction_id } = self { Some(transaction_id) } else { None }
    }
}

#[derive(Debug, Clone)]
pub struct BalanceChange {
    pub vault_id: VaultId,
    pub account_address: ComponentAddress,
    pub resource_address: ResourceAddress,
    pub revealed_balance_before: Amount,
    pub confidential_balance_before: Amount,
    pub revealed_balance_after: Amount,
    pub confidential_balance_after: Amount,
    // i128 to represent signed deltas — Amount is u128 and cannot go negative
    pub revealed_delta: i128,
    pub confidential_delta: i128,
    pub source: BalanceChangeSource,
    pub created_at: NaiveDateTime,
}
impl BalanceChange {
    pub fn new(
        vault_id: VaultId, account_address: ComponentAddress, resource_address: ResourceAddress,
        before_revealed: Amount, before_confidential: Amount,
        after_revealed: Amount, after_confidential: Amount,
        source: BalanceChangeSource,
    ) -> Self {
        // Use i128 arithmetic — direct Amount subtraction causes underflow panic
        // because Amount wraps u128 which cannot represent negative values.
        let revealed_delta =
            i128::from(after_revealed.as_u128()) - i128::from(before_revealed.as_u128());
        let confidential_delta =
            i128::from(after_confidential.as_u128()) - i128::from(before_confidential.as_u128());
        Self {
            vault_id, account_address, resource_address,
            revealed_balance_before: before_revealed,
            confidential_balance_before: before_confidential,
            revealed_balance_after: after_revealed,
            confidential_balance_after: after_confidential,
            revealed_delta, confidential_delta, source,
            created_at: chrono::Utc::now().naive_utc(),
        }
    }
    pub fn has_change(&self) -> bool {
        self.revealed_delta != 0 || self.confidential_delta != 0
    }
}
