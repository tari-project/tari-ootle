//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use time::PrimitiveDateTime;

pub struct Config<T> {
    pub key: String,
    pub value: T,
    pub is_encrypted: bool,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}
