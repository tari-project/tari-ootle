//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::VersionedSubstateIdRef;
use tari_dan_storage::consensus_models::SubstateChange;
use tari_engine_types::substate::{Substate, SubstateDiff};
use tari_transaction::TransactionId;

pub trait ReadableSubstateStore {
    type Error;

    fn get(&self, id: VersionedSubstateIdRef<'_>) -> Result<Substate, Self::Error>;
}

pub trait WriteableSubstateStore: ReadableSubstateStore {
    fn put(&mut self, change: SubstateChange) -> Result<(), Self::Error>;

    fn put_diff(&mut self, transaction_id: TransactionId, diff: &SubstateDiff) -> Result<(), Self::Error>;
}

pub trait SubstateStore: ReadableSubstateStore + WriteableSubstateStore {}

impl<T: ReadableSubstateStore + WriteableSubstateStore> SubstateStore for T {}
