//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::{AsChangeset, Identifiable, Queryable, dsl};
use time::PrimitiveDateTime;

use crate::schema::address_book;

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = address_book)]
pub struct AddressBookEntry {
    pub id: i32,
    pub name: String,
    pub address: String,
    pub note: Option<String>,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

/// Diesel changeset used by `address_book_update` so the three mutable columns
/// (`name`, `address`, `note`) are written in a single UPDATE statement. Each
/// field is wrapped in `Option` so callers can pass `None` to leave the column
/// untouched without issuing a separate query per field.
///
/// `note` is double-`Option` deliberately: the outer `Option` controls whether
/// the field is part of the UPDATE at all, and the inner `Option<&str>`
/// matches the nullable column so it can be set to `NULL` to clear a
/// previously-stored note.
///
/// `updated_at` is `dsl::now` (a zero-sized type, not `Option`) so every
/// UPDATE bumps the timestamp — matching the `StealthOutputUpdate` pattern in
/// `stealth_output.rs`. `dsl::now` cannot be wrapped in `Option` because it
/// is a SQL expression, not a value.
#[derive(Debug, AsChangeset)]
#[diesel(table_name = address_book)]
pub struct AddressBookEntryChangeset<'a> {
    pub name: Option<&'a str>,
    pub address: Option<&'a str>,
    pub note: Option<Option<&'a str>>,
    pub updated_at: dsl::now,
}
