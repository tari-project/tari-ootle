// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_common::configuration::Network;
use tari_transaction::TransactionBuilder;

pub fn transaction_builder() -> TransactionBuilder {
    TransactionBuilder::new().for_network(Network::LocalNet.as_byte())
}
