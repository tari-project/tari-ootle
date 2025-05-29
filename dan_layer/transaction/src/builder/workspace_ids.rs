//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use tari_template_lib::args::WorkspaceId;

#[derive(Debug, Clone, Default)]
pub struct WorkspaceIds {
    current_id: WorkspaceId,
    ids: HashMap<String, WorkspaceId>,
}

impl WorkspaceIds {
    pub fn new() -> Self {
        Self {
            current_id: 0,
            ids: HashMap::new(),
        }
    }

    fn next_id(&mut self) -> WorkspaceId {
        let id = self.current_id;
        self.current_id += 1;
        id
    }

    pub fn insert(&mut self, key: String) -> WorkspaceId {
        let id = self.next_id();
        self.ids.insert(key, id);
        id
    }

    pub fn get(&self, key: &str) -> Option<WorkspaceId> {
        self.ids.get(key).copied()
    }
}
