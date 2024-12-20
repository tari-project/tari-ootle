// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_dan_storage::StorageError;
use tari_engine_types::substate::SubstateId;

/// General trait for different state stores to do operations on substates.
pub trait SubstateStore {
    fn get_latest_version(&self, substate_id: &SubstateId) -> Result<u32, StorageError>;
}
