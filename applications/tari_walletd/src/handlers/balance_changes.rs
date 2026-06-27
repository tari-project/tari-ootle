//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::{handlers::{context::HandlerContext, error::HandlerError}, sdk::models::balance_change::BalanceChange};
use tari_ootle_transaction::transaction_id::TransactionId;
use tari_template_lib::models::{ComponentAddress, ResourceAddress};

#[derive(Debug, serde::Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../clients/wallet_daemon_client/src/bindings/")]
pub struct AccountsGetBalanceChangesRequest {
    pub account: ComponentAddress,
    pub resource_address: Option<ResourceAddress>,
    pub transaction_id: Option<TransactionId>,
    #[serde(default)] pub offset: u64,
    #[serde(default = "default_limit")] pub limit: u64,
}
fn default_limit() -> u64 { 20 }

#[derive(Debug, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../clients/wallet_daemon_client/src/bindings/")]
pub struct BalanceChangeEntry {
    pub vault_id: String,
    pub resource_address: String,
    pub revealed_balance_before: i64,
    pub confidential_balance_before: i64,
    pub revealed_balance_after: i64,
    pub confidential_balance_after: i64,
    pub revealed_delta: i64,
    pub confidential_delta: i64,
    pub source: String,
    pub transaction_id: Option<String>,
    pub created_at: String,
}

impl From<BalanceChange> for BalanceChangeEntry {
    fn from(ch: BalanceChange) -> Self {
        use crate::sdk::models::balance_change::BalanceChangeSource::*;
        let (src,txid) = match &ch.source {
            Transaction{transaction_id} => ("Transaction".into(), Some(transaction_id.to_string())),
            Scan     => ("Scan".into(), None),
            Recovery => ("Recovery".into(), None),
        };
        Self {
            vault_id: ch.vault_id.to_string(),
            resource_address: ch.resource_address.to_string(),
            revealed_balance_before: i64::from(ch.revealed_balance_before),
            confidential_balance_before: i64::from(ch.confidential_balance_before),
            revealed_balance_after: i64::from(ch.revealed_balance_after),
            confidential_balance_after: i64::from(ch.confidential_balance_after),
            revealed_delta: i64::from(ch.revealed_delta),
            confidential_delta: i64::from(ch.confidential_delta),
            source: src, transaction_id: txid,
            created_at: ch.created_at.to_string(),
        }
    }
}

#[derive(Debug, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../clients/wallet_daemon_client/src/bindings/")]
pub struct AccountsGetBalanceChangesResponse {
    pub changes: Vec<BalanceChangeEntry>,
    pub offset: u64,
    pub limit: u64,
}

pub async fn handle_get_balance_changes(
    context: &HandlerContext,
    token: Option<&axum_extra::headers::authorization::Bearer>,
    req: AccountsGetBalanceChangesRequest,
) -> Result<AccountsGetBalanceChangesResponse, HandlerError> {
    let _granted = context.check_auth(token)?;
    let sdk = context.wallet_sdk();
    let limit = req.limit.min(100);
    let changes = sdk.store().with_read_tx(|tx| {
        tx.balance_changes_get(&req.account, req.resource_address.as_ref(), req.transaction_id.as_ref(), req.offset, limit)
    }).map_err(HandlerError::from_storage)?;
    Ok(AccountsGetBalanceChangesResponse {
        changes: changes.into_iter().map(BalanceChangeEntry::from).collect(),
        offset: req.offset, limit,
    })
}
