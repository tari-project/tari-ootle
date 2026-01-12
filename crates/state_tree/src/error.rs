//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use jmt::storage::NodeKey;

#[derive(Debug, thiserror::Error)]
pub enum StateTreeError {
    #[error("JMT error: {0}")]
    JmtError(#[from] anyhow::Error),
    #[error("Attempted to insert a node with an existing key: {0:?}")]
    Conflict(NodeKey),
    #[error("Unexpected error: {0}")]
    Unexpected(String),
}
