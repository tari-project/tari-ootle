//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use crate::args::WorkspaceId;

#[derive(Debug, Clone, Default)]
pub struct WorkspaceIds {
    /// One past the highest allocated ID; `WorkspaceId::MAX + 1` represents exhaustion.
    next_id: u32,
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
        let id = self.next_id();
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

    pub fn advance_past(&mut self, id: WorkspaceId) {
        self.next_id = self.next_id.max(u32::from(id) + 1);
    }

    pub fn next_id(&self) -> WorkspaceId {
        self.next_id.try_into().expect("Workspace ID overflow")
    }

    /// Merges `other`'s workspace counter and named keys into `self`, returning the `WorkspaceId`
    /// offset by which `other`'s instruction workspace IDs should be shifted.
    ///
    /// # Panics
    ///
    /// - If the combined workspace ID count exceeds `WorkspaceId::MAX + 1`.
    /// - If a workspace key in `other` collides with one already registered in `self`.
    pub fn merge(&mut self, other: Self) -> WorkspaceId {
        if other.next_id == 0 {
            return 0;
        }
        let offset = self.next_id();
        let combined = self
            .next_id
            .checked_add(other.next_id)
            .filter(|&n| n <= u32::from(WorkspaceId::MAX) + 1)
            .expect("Workspace ID overflow during merge");
        for (name, id) in other.ids {
            assert!(
                !self.ids.contains_key(&name),
                "workspace key '{name}' collides during merge",
            );
            let shifted = id.checked_add(offset).expect("Workspace ID overflow during merge");
            self.ids.insert(name, shifted);
        }
        self.next_id = combined;
        offset
    }
}
