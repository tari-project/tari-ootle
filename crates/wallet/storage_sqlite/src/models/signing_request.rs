//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Diesel models for the `signing_requests` table. See the migration for the
//! invariants the columns carry.

use diesel::{Identifiable, Insertable, Queryable};
use time::PrimitiveDateTime;

use crate::schema::signing_requests;

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = signing_requests)]
pub struct SigningRequest {
    pub id: i32,
    pub unsigned_transaction: String,
    pub seal_public_key: String,
    pub key_id: String,
    pub signer_public_key: String,
    pub message_hash: String,
    pub memo: String,
    pub requester: String,
    pub status: String,
    pub signature: Option<String>,
    pub expires_at: PrimitiveDateTime,
    pub decided_at: Option<PrimitiveDateTime>,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = signing_requests)]
pub struct NewSigningRequest<'a> {
    pub unsigned_transaction: &'a str,
    pub seal_public_key: &'a str,
    pub key_id: &'a str,
    pub signer_public_key: &'a str,
    pub message_hash: &'a str,
    pub memo: &'a str,
    pub requester: &'a str,
    pub status: &'a str,
    pub expires_at: PrimitiveDateTime,
}
