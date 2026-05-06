// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

#[derive(Debug, Clone)]
pub struct ApiKeyModel {
    pub id: String,
    pub name: String,
    pub key_hash: Vec<u8>,
    pub permissions: Vec<String>,
    pub created_at: u64,
    pub last_used_at: Option<u64>,
    pub revoked_at: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct NewApiKeyModel {
    pub id: String,
    pub name: String,
    pub key_hash: Vec<u8>,
    pub permissions: Vec<String>,
}
