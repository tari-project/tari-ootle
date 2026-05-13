//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use time::PrimitiveDateTime;

#[derive(Debug, Clone, PartialEq, Eq)]
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
