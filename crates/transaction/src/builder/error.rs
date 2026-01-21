//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::indexed_value::IndexedValueError;

use crate::builder::named_args::ParseWorkspaceKeyError;

#[derive(Debug, thiserror::Error)]
pub enum BuilderError {
    #[error("Failed to parse workspace key: {0}")]
    ParseWorkspaceKeyError(#[from] ParseWorkspaceKeyError),
    #[error("Workspace key not found: {0}")]
    WorkspaceKeyNotFound(String),
    #[error("Indexed value error: {0}")]
    IndexedValueError(#[from] IndexedValueError),
}
