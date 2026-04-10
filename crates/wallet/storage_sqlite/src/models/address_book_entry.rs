//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::{Identifiable, Queryable};
use time::PrimitiveDateTime;

use crate::schema::address_book;

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = address_book)]
pub struct AddressBookEntry {
    pub id: i32,
    pub name: String,
    pub address: String,
    pub memo: Option<String>,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}
