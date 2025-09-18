//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::Queryable;

#[derive(Debug, Clone, Queryable)]
pub struct UtxoProcessQueue {
    pub _id: i32,
    pub account_key_index: i64,
    pub resource_address: String,
    pub utxo_tag: i32,
    pub public_nonce: String,
    pub _created_at: time::PrimitiveDateTime,
}
