use chrono::NaiveDateTime;
use tari_ootle_transaction::transaction_id::TransactionId;
use tari_template_lib::models::{Amount, ComponentAddress, ResourceAddress, VaultId};

/// Source of a vault balance change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BalanceChangeSource {
    /// A finalised transaction changed this vault.
    Transaction { transaction_id: TransactionId },
    /// An account re-scan discovered funds not previously attributed to a tx.
    Scan,
    /// Confidential UTXO recovery produced new spendable outputs.
    Recovery,
}

impl BalanceChangeSource {
    pub fn source_tag(&self) -> i32 {
        match self {
            Self::Transaction { .. } => 0,
            Self::Scan => 1,
            Self::Recovery => 2,
        }
    }

    pub fn transaction_id(&self) -> Option<&TransactionId> {
        if let Self::Transaction { transaction_id } = self {
            Some(transaction_id)
        } else {
            None
        }
    }
}

/// A single recorded balance change for one vault.
#[derive(Debug, Clone)]
pub struct BalanceChange {
    pub vault_id: VaultId,
    pub account_address: ComponentAddress,
    pub resource_address: ResourceAddress,
    pub revealed_balance_before: Amount,
    pub confidential_balance_before: Amount,
    pub revealed_balance_after: Amount,
    pub confidential_balance_after: Amount,
    pub revealed_delta: Amount,
    pub confidential_delta: Amount,
    pub source: BalanceChangeSource,
    pub created_at: NaiveDateTime,
}

impl BalanceChange {
    pub fn new(
        vault_id: VaultId,
        account_address: ComponentAddress,
        resource_address: ResourceAddress,
        before_revealed: Amount,
        before_confidential: Amount,
        after_revealed: Amount,
        after_confidential: Amount,
        source: BalanceChangeSource,
    ) -> Self {
        Self {
            vault_id,
            account_address,
            resource_address,
            revealed_balance_before: before_revealed,
            confidential_balance_before: before_confidential,
            revealed_balance_after: after_revealed,
            confidential_balance_after: after_confidential,
            revealed_delta: after_revealed - before_revealed,
            confidential_delta: after_confidential - before_confidential,
            source,
            created_at: chrono::Utc::now().naive_utc(),
        }
    }

    /// Returns true only when at least one balance actually changed.
    pub fn has_change(&self) -> bool {
        self.revealed_delta != Amount::zero() || self.confidential_delta != Amount::zero()
    }
}
