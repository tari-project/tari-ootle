//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use crate::args::WorkspaceId;

#[derive(Debug, Clone, Default)]
pub struct WorkspaceIds {
    next_id: WorkspaceId,
    ids: HashMap<String, WorkspaceId>,
}

impl WorkspaceIds {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            ids: HashMap::new(),
        }
    }

    fn progress_to_next_id(&mut self) -> WorkspaceId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn insert(&mut self, key: String) -> WorkspaceId {
        let id = self.progress_to_next_id();
        self.ids.insert(key, id);
        id
    }

    pub fn get(&self, key: &str) -> Option<WorkspaceId> {
        self.ids.get(key).copied()
    }

    pub fn set_next_id(&mut self, id: WorkspaceId) {
        self.next_id = id;
    }

    pub fn next_id(&self) -> WorkspaceId {
        self.next_id
    }
}
