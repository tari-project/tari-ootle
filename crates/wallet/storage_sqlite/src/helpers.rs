//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use time::{PrimitiveDateTime, UtcDateTime};

pub fn now() -> PrimitiveDateTime {
    let now = UtcDateTime::now();
    PrimitiveDateTime::new(now.date(), now.time())
}
