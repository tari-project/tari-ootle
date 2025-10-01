// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use log::info;
use tari_ootle_common_types::Network;
use tari_transaction::TransactionBuilder;

pub fn transaction_builder() -> TransactionBuilder {
    TransactionBuilder::new().for_network(Network::LocalNet.as_byte())
}

pub fn cucumber_log<T: AsRef<str>>(msg: T) {
    if option_env!("CUC_DEBUG") == Some("1") {
        eprintln!("🥒 {}", msg.as_ref());
    }
    info!(target: "cucumber", "🥒 {}", msg.as_ref());
}
