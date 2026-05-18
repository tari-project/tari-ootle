//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_consensus::consensus_constants::ConsensusConstants;
use tari_ootle_common_types::{NodeAddressable, optional::Optional};
use tari_ootle_transaction::Network;
use tari_state_store_rocksdb::{
    codecs::ByteColumn,
    column_families::bookkeeping::DatabaseMigrationVersion,
    writer::RocksDbStateStoreWriteTransaction,
};

use crate::genesis_state::create_genesis_state;

pub mod common;
pub mod v1;
pub mod v2;

const LOG_TARGET: &str = "tari::validator::migrations";

pub fn migrate<TAddr: NodeAddressable + 'static>(
    tx: &mut RocksDbStateStoreWriteTransaction<'_, TAddr>,
    network: Network,
    consensus_constants: &ConsensusConstants,
) -> anyhow::Result<()> {
    const OPERATION: &str = "migrate";
    const CURRENT_VERSION: u64 = 2;
    let maybe_version = {
        let db = tx.db();
        db.cf(DatabaseMigrationVersion)?
            .get(&ByteColumn, OPERATION)
            .optional()?
    };

    match maybe_version {
        Some(mut version) => {
            info!(
                target: LOG_TARGET,
                "🔨 Migration required from version {version} to {CURRENT_VERSION}"
            );
            while version < CURRENT_VERSION {
                match version {
                    0 => {
                        let db = tx.db();
                        v1::migrate(&db, network)?;
                    },
                    1 => {
                        v2::migrate(tx, network, consensus_constants.num_preshards)?;
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
                tx.db()
                    .cf(DatabaseMigrationVersion)?
                    .put(&ByteColumn, &version, OPERATION)?;
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
