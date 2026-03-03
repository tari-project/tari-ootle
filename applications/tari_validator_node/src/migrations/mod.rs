//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_consensus::consensus_constants::ConsensusConstants;
use tari_ootle_common_types::{Network, NodeAddressable, optional::Optional};
use tari_state_store_rocksdb::{
    codecs::ByteColumn,
    column_families::bookkeeping::DatabaseMigrationVersion,
    writer::RocksDbStateStoreWriteTransaction,
};

use crate::genesis_state::create_genesis_state;

pub mod v1;

const LOG_TARGET: &str = "tari::validator::migrations";

pub fn migrate<TAddr: NodeAddressable + 'static>(
    tx: &mut RocksDbStateStoreWriteTransaction<'_, TAddr>,
    network: Network,
    consensus_constants: &ConsensusConstants,
) -> anyhow::Result<()> {
    const OPERATION: &str = "migrate";
    const CURRENT_VERSION: u64 = 1;
    let maybe_version = {
        let db = tx.db();
        db.cf(DatabaseMigrationVersion)?
            .get(&ByteColumn, OPERATION)
            .optional()?
    };

    match maybe_version {
        Some(mut version) => {
            let db = tx.db();
            info!(
                target: LOG_TARGET,
                "🔨 Migration required from version {version} to {CURRENT_VERSION}"
            );
            while version < CURRENT_VERSION {
                match version {
                    0 => {
                        v1::migrate(&db, network)?;
                    },
                    _ => unreachable!("version somehow went over CURRENT_VERSION. This is a bug"),
                }
                info!(
                    target: LOG_TARGET,
                    "🔨 Migration from {} to {} complete",
                    version,
                    version + 1
                );
                version += 1;
                db.cf(DatabaseMigrationVersion)?.put(&ByteColumn, &version, OPERATION)?;
            }
        },
        None => {
            info!(
                target: LOG_TARGET,
                "🌱 Fresh database - adding genesis state"
            );
            create_genesis_state(tx, network, consensus_constants.num_preshards)?;
            tx.db()
                .cf(DatabaseMigrationVersion)?
                .put(&ByteColumn, &CURRENT_VERSION, OPERATION)?;
        },
    }

    Ok(())
}
