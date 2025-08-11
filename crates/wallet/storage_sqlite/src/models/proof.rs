//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::{Identifiable, Queryable};
use time::PrimitiveDateTime;

use crate::schema::output_locks;

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = output_locks)]
pub struct OutputLock {
    pub id: i32,
    pub resource_address: String,
    pub vault_id: Option<i32>,
    pub transaction_hash: Option<String>,
    pub locked_revealed_amount: i64,
    pub created_at: PrimitiveDateTime,
}
