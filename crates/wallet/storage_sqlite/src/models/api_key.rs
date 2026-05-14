//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::prelude::*;

use crate::schema::api_keys;

#[derive(Debug, Clone, Queryable, Identifiable, Selectable)]
#[diesel(table_name = api_keys)]
pub struct ApiKey {
    pub id: i64,
    pub name: String,
    pub key_hash: Vec<u8>,
    pub permissions: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = api_keys)]
pub struct NewApiKey<'a> {
    pub name: &'a str,
    pub key_hash: &'a [u8],
    pub permissions: &'a str,
    pub created_at: i64,
}
