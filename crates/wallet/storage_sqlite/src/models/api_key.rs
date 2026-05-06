// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_sdk::models::ApiKeyModel;
use time::PrimitiveDateTime;

use crate::schema::wallet_api_keys;

#[derive(Debug, Clone, Queryable, Identifiable, Selectable)]
#[diesel(table_name = wallet_api_keys)]
#[diesel(primary_key(id))]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    pub key_hash: Vec<u8>,
    pub permissions: Vec<u8>,
    pub last_used_at: Option<PrimitiveDateTime>,
    pub revoked_at: Option<PrimitiveDateTime>,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

impl TryFrom<ApiKey> for ApiKeyModel {
    type Error = serde_json::Error;

    fn try_from(value: ApiKey) -> Result<Self, Self::Error> {
        let permissions = serde_json::from_slice(&value.permissions)?;
        Ok(Self {
            id: value.id,
            name: value.name,
            key_hash: value.key_hash,
            permissions,
            created_at: primitive_datetime_to_secs(value.created_at),
            last_used_at: value.last_used_at.map(primitive_datetime_to_secs),
            revoked_at: value.revoked_at.map(primitive_datetime_to_secs),
        })
    }
}

fn primitive_datetime_to_secs(value: PrimitiveDateTime) -> u64 {
    value.assume_utc().unix_timestamp().max(0) as u64
}
