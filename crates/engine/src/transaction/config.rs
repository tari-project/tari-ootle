//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

#[derive(Clone, Debug)]
pub struct TransactionProcessorConfig {
    pub template_binary_max_size_bytes: usize,
}

impl TransactionProcessorConfig {
    pub const fn new() -> Self {
        Self {
            template_binary_max_size_bytes: 1000 * 1000 * 5, // 5MB
        }
    }

    pub const fn with_template_binary_max_size_bytes(mut self, max_size: usize) -> Self {
        self.template_binary_max_size_bytes = max_size;
        self
    }
}

impl Default for TransactionProcessorConfig {
    fn default() -> Self {
        Self::new()
    }
}
