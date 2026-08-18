// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_address::Network;
use tari_ootle_transaction::{Epoch, TransactionBuilder};

/// The builder is stamped with a random nonce so that repeated identical calls (concurrent or
/// sequential steps invoking the same method with unversioned inputs) build distinct
/// transactions — the transaction id excludes the seal signature, so identical bodies sealed by
/// the same key would otherwise be a single transaction.
pub fn transaction_builder() -> TransactionBuilder {
    TransactionBuilder::new(Network::LocalNet, DEFAULT_TEST_MAX_EPOCH).with_nonce(rand::random())
}

/// Validity window for transactions built by the test harness. A cucumber network starts at a low
/// epoch and a scenario is short, so this outlives every run while staying well inside the
/// network's `max_transaction_validity_epochs` ceiling. Scenarios that pin a specific window
/// override it with `with_max_epoch`.
pub const DEFAULT_TEST_MAX_EPOCH: Epoch = Epoch(100);

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
