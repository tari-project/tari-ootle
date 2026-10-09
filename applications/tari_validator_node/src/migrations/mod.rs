//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Database state migrations.
//!
//! Migrations upgrade the persisted state of *already-running* nodes when a change would otherwise
//! only take effect for freshly bootstrapped databases. Each database records the schema version it
//! was last brought up to ([`DatabaseMigrationVersion`]); [`migrate`] runs the steps between that
//! stored version and [`CURRENT_VERSION`]. A fresh database skips migrations entirely - it is stamped
//! directly with `CURRENT_VERSION` once the genesis state is laid down.
//!
//! # Adding a migration
//!
//! To take the schema from version `N` to `N + 1`:
//!
//! 1. Add `v{N+1}.rs` with `pub fn migrate(...) -> ...` performing the upgrade, and declare it here with `mod v{N+1};`.
//! 2. Bump `CURRENT_SCHEMA_VERSION` (in the state store, beside `DatabaseMigrationVersion`) to `N + 1`.
//! 3. Apply it from [`migrate`]'s step loop, in an arm keyed by the version it upgrades *from*. The loop steps the
//!    stored version up one migration at a time, persisting each version it reaches:
//!
//! ```ignore
//! while version < CURRENT_VERSION {
//!     match version {
//!         2 => v3::migrate(tx)?,
//!         other => anyhow::bail!("no migration defined for database version {other}"),
//!     }
//!     version += 1;
//!     tx.db().cf(DatabaseMigrationVersion)?.put(&ByteColumn, &version, OPERATION)?;
//! }
//! ```
//!
//! A migration must be able to run against a database at any earlier supported version, so it may not assume the
//! current schema of anything it does not itself write.
//!
//! IMPORTANT: a migration that creates or mutates substates must write them to the per-shard state
//! tree (JMT), not only the substate store - otherwise they have no inclusion proof and verified
//! reads of them fail. Mirror [`crate::genesis_state::create_genesis_state`], which commits each
//! substate to both the store and the state tree. (Note that, unlike genesis, adding state-tree
//! entries to a live chain shifts its state root, so such a migration is itself consensus-affecting.)

use log::*;
use tari_consensus::consensus_constants::ConsensusConstants;
use tari_ootle_app_utilities::genesis_governance::GenesisCouncil;
use tari_ootle_common_types::{NodeAddressable, optional::Optional};
use tari_ootle_transaction::Network;
use tari_state_store_rocksdb::{
    codecs::ByteColumn,
    // The version constant lives in the state store because it describes the on-disk schema, and tools that write to
    // a database directly must check it before doing so. Bump it there and apply the upgrade in `migrate`.
    column_families::bookkeeping::{CURRENT_SCHEMA_VERSION as CURRENT_VERSION, DatabaseMigrationVersion},
    writer::RocksDbStateStoreWriteTransaction,
};

use crate::genesis_state::create_genesis_state;

mod v1;
mod v2;

const LOG_TARGET: &str = "tari::validator_node::migrations";

pub fn migrate<TAddr: NodeAddressable + 'static>(
    tx: &mut RocksDbStateStoreWriteTransaction<'_, TAddr>,
    network: Network,
    consensus_constants: &ConsensusConstants,
    genesis_council: &GenesisCouncil,
) -> anyhow::Result<()> {
    const OPERATION: &str = "migrate";

    let maybe_version = {
        let db = tx.db();
        db.cf(DatabaseMigrationVersion)?
            .get(&ByteColumn, OPERATION)
            .optional()?
    };

    match maybe_version {
        Some(version) if version.cmp(&CURRENT_VERSION).is_ge() => {
            debug!(
                target: LOG_TARGET,
                "Database already bootstrapped at migration version {version} (current {CURRENT_VERSION})"
            );
        },
        Some(mut version) => {
            while version < CURRENT_VERSION {
                info!(
                    target: LOG_TARGET,
                    "Migrating database from version {version} to {}", version + 1
                );
                match version {
                    0 => v1::migrate(tx)?,
                    1 => v2::migrate(tx)?,
                    other => anyhow::bail!(
                        "Database is at migration version {other}, and no migration upgrades it to version {}. Delete \
                         the database and resync.",
                        other + 1
                    ),
                }
                version += 1;
                tx.db()
                    .cf(DatabaseMigrationVersion)?
                    .put(&ByteColumn, &version, OPERATION)?;
            }
        },
        // A fresh database: lay down the genesis state and stamp the current version.
        None => {
            info!(target: LOG_TARGET, "🌱 Fresh database - adding genesis state");
            create_genesis_state(tx, network, consensus_constants.num_preshards, genesis_council)?;
            tx.db()
                .cf(DatabaseMigrationVersion)?
                .put(&ByteColumn, &CURRENT_VERSION, OPERATION)?;
        },
    }

    Ok(())
}
