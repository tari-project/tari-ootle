//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::{Identifiable, Queryable};
use tari_dan_storage::time::PrimitiveDateTime;

use crate::schema::config;

#[derive(Debug, Clone, Identifiable, Queryable)]
#[diesel(table_name = config)]
pub struct Config {
    pub id: i32,
    pub key: String,
    pub value: String,
    pub is_encrypted: bool,
    pub updated_at: PrimitiveDateTime,
    pub created_at: PrimitiveDateTime,
}
