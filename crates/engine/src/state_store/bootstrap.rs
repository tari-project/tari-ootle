//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use crate::state_store::memory::MemoryStateStore;

pub fn new_memory_store() -> MemoryStateStore {
    MemoryStateStore::new()
}
