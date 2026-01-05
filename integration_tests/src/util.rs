// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::Network;
use tari_transaction::TransactionBuilder;

pub fn transaction_builder() -> TransactionBuilder {
    TransactionBuilder::new(Network::LocalNet)
}

#[macro_export]
macro_rules! cucumber_log {
    ($msg:expr) => {{
        let msg = $msg;
        if option_env!("CUC_DEBUG") == Some("1") {
            eprintln!("🥒 [{}:{}] {}", file!(), line!(), msg);
        }
        log::info!(target: "cucumber", "🥒 [{}:{}] {}", file!(), line!(), msg);
    }};
}
