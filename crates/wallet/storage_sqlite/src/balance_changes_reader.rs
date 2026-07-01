//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use chrono::NaiveDateTime;
use diesel::prelude::*;
use tari_ootle_transaction::transaction_id::TransactionId;
use tari_template_lib::models::{Amount, ComponentAddress, ResourceAddress, VaultId};
use crate::models::balance_change::{BalanceChange, BalanceChangeSource};

pub fn balance_changes_get(
    conn: &mut SqliteConnection,
    account_address: &ComponentAddress,
    resource_address: Option<&ResourceAddress>,
    transaction_id: Option<&TransactionId>,
    offset: u64,
    limit: u64,
) -> QueryResult<Vec<BalanceChange>> {
    use crate::schema::account_balance_changes::dsl as bc;
    let ab = account_address.as_bytes().to_vec();
    let mut q = bc::account_balance_changes
        .filter(bc::account_address.eq(&ab))
        .order(bc::created_at.desc())
        .into_boxed();
    if let Some(r) = resource_address { q = q.filter(bc::resource_address.eq(r.as_bytes().to_vec())); }
    if let Some(t) = transaction_id { q = q.filter(bc::transaction_id.eq(t.as_bytes().to_vec())); }

    // NaiveDateTime for created_at — Diesel SQLite maps Timestamp to NaiveDateTime.
    // Using String causes a runtime deserialization error (gemini review fix).
    type Row = (i64, Vec<u8>, Vec<u8>, Vec<u8>, i64, i64, i64, i64, i64, i64, i32, Option<Vec<u8>>, NaiveDateTime);

    let rows: Vec<Row> = q.select((
        bc::id, bc::vault_id, bc::account_address, bc::resource_address,
        bc::revealed_balance_before, bc::confidential_balance_before,
        bc::revealed_balance_after, bc::confidential_balance_after,
        bc::revealed_delta, bc::confidential_delta,
        bc::source, bc::transaction_id, bc::created_at,
    )).limit(limit as i64).offset(offset as i64).load(conn)?;

    rows.into_iter().map(|(_, vid, _, rid, rb, cb, ra, ca, rd, cd, src, txb, created_at)| {
        let source = match src {
            0 => {
                let tx = txb.as_deref().and_then(|b| TransactionId::try_from(b).ok()).unwrap_or_default();
                BalanceChangeSource::Transaction { transaction_id: tx }
            },
            1 => BalanceChangeSource::Scan,
            _ => BalanceChangeSource::Recovery,
        };
        use diesel::result::Error::DeserializationError as DE;
        Ok(BalanceChange {
            vault_id: VaultId::try_from(vid.as_slice()).map_err(|e| DE(Box::new(e)))?,
            account_address: ComponentAddress::from_bytes(&ab).map_err(|e| DE(Box::new(e)))?,
            resource_address: ResourceAddress::from_bytes(&rid).map_err(|e| DE(Box::new(e)))?,
            revealed_balance_before: Amount::from(rb as u128),
            confidential_balance_before: Amount::from(cb as u128),
            revealed_balance_after: Amount::from(ra as u128),
            confidential_balance_after: Amount::from(ca as u128),
            revealed_delta: rd as i128,
            confidential_delta: cd as i128,
            source, created_at,
        })
    }).collect()
}
