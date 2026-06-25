use diesel::prelude::*;
use crate::models::balance_change::{BalanceChange, BalanceChangeSource};

/// Insert a balance change entry, ignoring duplicates (idempotent).
pub fn balance_changes_insert(conn: &mut SqliteConnection, change: &BalanceChange) -> QueryResult<usize> {
    use crate::schema::account_balance_changes;
    let tx_id_bytes: Option<Vec<u8>> = change.source.transaction_id().map(|id| id.as_bytes().to_vec());
    diesel::insert_or_ignore_into(account_balance_changes::table)
        .values((
            account_balance_changes::vault_id.eq(change.vault_id.as_bytes()),
            account_balance_changes::account_address.eq(change.account_address.as_bytes()),
            account_balance_changes::resource_address.eq(change.resource_address.as_bytes()),
            account_balance_changes::revealed_balance_before.eq(i64::from(change.revealed_balance_before)),
            account_balance_changes::confidential_balance_before.eq(i64::from(change.confidential_balance_before)),
            account_balance_changes::revealed_balance_after.eq(i64::from(change.revealed_balance_after)),
            account_balance_changes::confidential_balance_after.eq(i64::from(change.confidential_balance_after)),
            account_balance_changes::revealed_delta.eq(i64::from(change.revealed_delta)),
            account_balance_changes::confidential_delta.eq(i64::from(change.confidential_delta)),
            account_balance_changes::source.eq(change.source.source_tag()),
            account_balance_changes::transaction_id.eq(tx_id_bytes),
        ))
        .execute(conn)
}
