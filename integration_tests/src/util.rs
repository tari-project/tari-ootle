// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_address::Network;
use tari_ootle_transaction::TransactionBuilder;

pub fn transaction_builder() -> TransactionBuilder {
    TransactionBuilder::new(Network::LocalNet)
}

#[macro_export]
macro_rules! cucumber_log {
    ($($msg:tt)*) => {{
        let msg = format_args!($($msg)*);
        if option_env!("CUC_DEBUG") == Some("1") {
            eprintln!("🥒 [{}:{}] {}", file!(), line!(), msg);
        }
        log::info!(target: "cucumber", "🥒 [{}:{}] {}", file!(), line!(), msg);
    }};
}
