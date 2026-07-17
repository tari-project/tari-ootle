//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Diesel models for the `transaction_requests` table.
//!
//! A transaction request is the artifact a human approves: a frozen
//! `UnsignedTransaction` plus the hash the approval commits to. See the
//! migration for the invariants the columns carry.

use diesel::{Identifiable, Insertable, Queryable};
use time::PrimitiveDateTime;

use crate::schema::transaction_requests;

/// One row of the `transaction_requests` table.
#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = transaction_requests)]
pub struct TransactionRequest {
    pub id: i32,
    pub unsigned_transaction: String,
    pub seal_signer: String,
    pub other_signers: String,
    pub signatures: String,
    pub lock_ids: String,
    pub requested_by: Option<String>,
    pub status: String,
    pub transaction_id: Option<String>,
    pub expires_at: PrimitiveDateTime,
    pub approved_at: Option<PrimitiveDateTime>,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

/// Insert shape for [`TransactionRequest`]. `status` is not settable: a
/// request is always born Pending, and every later transition goes through a
/// guarded update.
#[derive(Debug, Insertable)]
#[diesel(table_name = transaction_requests)]
pub struct NewTransactionRequest<'a> {
    pub unsigned_transaction: &'a str,
    pub seal_signer: &'a str,
    pub other_signers: &'a str,
    pub signatures: &'a str,
    pub lock_ids: &'a str,
    pub requested_by: Option<&'a str>,
    pub status: &'a str,
    pub expires_at: PrimitiveDateTime,
}
