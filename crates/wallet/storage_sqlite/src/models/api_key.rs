//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::{Identifiable, Queryable};
use tari_ootle_wallet_sdk::models::ApiKey as ApiKeyModel;
use time::PrimitiveDateTime;

use crate::schema::api_keys;

#[derive(Debug, Clone, Identifiable, Queryable)]
#[diesel(table_name = api_keys)]
pub struct ApiKey {
    pub id: i32,
    pub name: String,
    pub key_hash: String,
    pub permissions: String,
    pub created_at: PrimitiveDateTime,
    pub last_used_at: Option<PrimitiveDateTime>,
    pub expires_at: Option<PrimitiveDateTime>,
    pub revoked_at: Option<PrimitiveDateTime>,
}

impl From<ApiKey> for ApiKeyModel {
    fn from(value: ApiKey) -> Self {
        Self {
            id: value.id,
            name: value.name,
            key_hash: value.key_hash,
            permissions: value.permissions,
            created_at: value.created_at,
            last_used_at: value.last_used_at,
            expires_at: value.expires_at,
            revoked_at: value.revoked_at,
        }
    }
}
