//! Changes required in scanner.rs to record balance changes.
//!
//! 1. Add `source: BalanceChangeSource` parameter to `refresh_vault`.
//!
//! 2. At scanner.rs ~L252 (compare-and-write block), after calling
//!    `accounts_api.update_vault_balance(...)`, add:
//!
//!    ```rust
//!    let change = BalanceChange::new(
//!        vault_id, account_address, vault_model.resource_address,
//!        vault_model.revealed_balance, vault_model.confidential_balance,
//!        new_balance, new_confidential_balance,
//!        source.clone(),
//!    );
//!    if change.has_change() {
//!        accounts_api.insert_balance_change(&change)?;
//!    }
//!    ```
//!
//! 3. Scan call site (L143): pass `BalanceChangeSource::Scan`.
//!
//! 4. Transaction call sites (L516, L590): pass
//!    `BalanceChangeSource::Transaction { transaction_id: tx_id }`.
//!
//! This module documents the patch; actual changes are in scanner.rs.
pub const PATCH_NOTES: &str = "see scanner.rs diff in this PR";
