//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Version 1 records which uncommitted blocks have a saved change set, and consensus refuses to commit a block
//! without one. Version 0 kept no such record, so this migration records one for every uncommitted block the database
//! holds, which commits them as version 0 would have. That includes a block this node declined before the upgrade:
//! version 0 saved no change set for such a block, and it still commits with none of its state changes applied. Only
//! blocks already stored at the upgrade are affected; every block stored after it is recorded only when its change
//! set is saved.

use log::*;
use tari_ootle_common_types::NodeAddressable;
use tari_ootle_storage::{Ordering, StateStoreWriteTransaction};
use tari_state_store_rocksdb::{column_families::chain::PendingChainIndex, writer::RocksDbStateStoreWriteTransaction};

const LOG_TARGET: &str = "tari::validator_node::migrations::v1";

pub fn migrate<TAddr: NodeAddressable + 'static>(
    tx: &mut RocksDbStateStoreWriteTransaction<'_, TAddr>,
) -> anyhow::Result<()> {
    const OPERATION: &str = "migrate_v1";

    let pending = tx
        .db()
        .cf(PendingChainIndex)?
        .key_iterator(Ordering::Ascending, OPERATION)
        .collect::<Result<Vec<_>, _>>()?;
    for block_id in &pending {
        tx.blocks_set_executed(block_id)?;
    }

    info!(
        target: LOG_TARGET,
        "Recorded a saved change set for {} uncommitted block(s)",
        pending.len()
    );
    Ok(())
}
