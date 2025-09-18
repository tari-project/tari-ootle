//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::Network;

#[derive(Clone, Debug)]
pub struct TransactionProcessorConfig {
    pub network: Network,
    pub template_binary_max_size_bytes: usize,
}

impl TransactionProcessorConfig {
    pub const fn new(network: Network) -> Self {
        Self {
            network,
            template_binary_max_size_bytes: 1000 * 1000 * 5, // 5MB
        }
    }

    pub const fn with_template_binary_max_size_bytes(mut self, max_size: usize) -> Self {
        self.template_binary_max_size_bytes = max_size;
        self
    }
}
