// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

#[derive(Debug, Clone, Queryable, Insertable, Identifiable)]
#[diesel(table_name = crate::schema::api_keys)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    pub key_hash: String,
    pub scopes: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub last_used: Option<i64>,
    pub revoked: i32,
}
