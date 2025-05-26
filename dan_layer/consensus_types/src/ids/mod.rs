//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod block_id;
mod macros;

pub use block_id::*;

crate::create_hash_type!(
    ///The ID of a Proposal Certificate
    QcId
);

crate::create_hash_type!(
    ///The ID of a Timeout Certificate
    TcId
);
