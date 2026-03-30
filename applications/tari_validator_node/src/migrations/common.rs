//    Copyright 2026 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use serde::Serialize;
use tari_engine_types::substate::{SubstateId, SubstateValue};
use tari_ootle_common_types::{Epoch, NodeAddressable, NumPreshards, VersionedSubstateIdRef};
use tari_ootle_storage::{
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
    consensus_models::{SubstateRecord, SubstateTransition, SubstateUpdateBatch},
};

pub(super) fn create_substate<TTx, TId, TVal>(
    tx: &mut TTx,
    num_preshards: NumPreshards,
    substate_id: TId,
    value: TVal,
) -> Result<(), StorageError>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
    TTx::Addr: NodeAddressable + Serialize,
    TId: Into<SubstateId>,
    TVal: Into<SubstateValue>,
{
    let substate_id = substate_id.into();
    let shard = VersionedSubstateIdRef::new(&substate_id, 0).to_shard(num_preshards);
    let mut batch = SubstateUpdateBatch::new(Epoch::zero());
    batch.with_transition(shard, 0).push(SubstateTransition::Up {
        id: substate_id,
        version: 0,
        substate_or_hash: value.into().into(),
    });

    SubstateRecord::commit_batch(tx, batch)?;

    Ok(())
}
