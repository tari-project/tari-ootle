//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRecord {
    pub id: i64,
    pub name: String,
    pub key_hash: Vec<u8>,
    pub permissions: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
    pub revoked_at: Option<i64>,
}

impl ApiKeyRecord {
    pub fn is_revoked(&self) -> bool {
        self.revoked_at.is_some()
    }
}
