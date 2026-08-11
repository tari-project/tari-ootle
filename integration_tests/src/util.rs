// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_address::Network;
use tari_ootle_transaction::TransactionBuilder;

/// The builder is stamped with a random nonce so that repeated identical calls (concurrent or
/// sequential steps invoking the same method with unversioned inputs) build distinct
/// transactions — the transaction id excludes the seal signature, so identical bodies sealed by
/// the same key would otherwise be a single transaction.
pub fn transaction_builder() -> TransactionBuilder {
    TransactionBuilder::new(Network::LocalNet).with_nonce(rand::random())
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
