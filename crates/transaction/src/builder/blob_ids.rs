//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use crate::BlobIndex;

/// Maps caller-supplied blob names to the `BlobIndex` they were registered at on the
/// transaction. Mirrors `WorkspaceIds` for blob references.
#[derive(Debug, Clone, Default)]
pub struct BlobIds {
    ids: HashMap<String, BlobIndex>,
}

impl BlobIds {
    pub fn new() -> Self {
        Self { ids: HashMap::new() }
    }

    pub fn insert(&mut self, key: String, idx: BlobIndex) {
        self.ids.insert(key, idx);
    }

    pub fn get(&self, key: &str) -> Option<BlobIndex> {
        self.ids.get(key).copied()
    }
}
