//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use tari_common_types::types::FixedHash;

#[derive(Debug, Clone)]
pub struct BlockHeaderData {
    pub height: u64,
    pub hash: FixedHash,
    pub kernel_merkle_root: FixedHash,
    pub validator_node_merkle_root: FixedHash,
}

impl Display for BlockHeaderData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "BlockHeaderData {{ height: {}, hash: {}, kernel_merkle_root: {}, validator_node_merkle_root: {} }}",
            self.height, self.hash, self.kernel_merkle_root, self.validator_node_merkle_root,
        )
    }
}
