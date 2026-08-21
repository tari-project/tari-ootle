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
//! 2. Bump [`CURRENT_VERSION`] to `N + 1`.
//! 3. Add the arm that applies it to the step loop in [`migrate`], keyed by the version it upgrades *from*:
//!
//! ```ignore
//! match version {
//!     0 => v1::migrate(tx)?,
//!     1 => v2::migrate(tx, network)?,
//!     other => unreachable!("no migration defined for database version {other}"),
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

mod v1;

use std::time::Instant;

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

const LOG_TARGET: &str = "tari::validator::migrations";

/// The on-disk state schema version stamped onto a freshly bootstrapped database.
///
/// Bump this and apply the upgrade in [`migrate`] whenever the persisted state must change for
/// already-running nodes. It was reset to 0 with the genesis-in-state-tree testnet reset, and is at 1
/// for the substate lock index prefix ([`v1`]).
const CURRENT_VERSION: u64 = 1;

pub fn migrate<TAddr: NodeAddressable + 'static>(
    tx: &mut RocksDbStateStoreWriteTransaction<'_, TAddr>,
    network: Network,
    consensus_constants: &ConsensusConstants,
) -> anyhow::Result<()> {
    const OPERATION: &str = "migrate";

    let maybe_version = {
        let db = tx.db();
        db.cf(DatabaseMigrationVersion)?
            .get(&ByteColumn, OPERATION)
            .optional()?
    };

    match maybe_version {
        // An already-bootstrapped database: step it up to `CURRENT_VERSION`, applying each upgrade and
        // persisting the new version as it goes.
        Some(version) if version >= CURRENT_VERSION => {
            debug!(
                target: LOG_TARGET,
                "Database already bootstrapped at migration version {version} (current {CURRENT_VERSION})"
            );
        },
        Some(mut version) => {
            info!(
                target: LOG_TARGET,
                "🔀 Migrating database from version {version} to {CURRENT_VERSION}"
            );
            let timer = Instant::now();
            while version < CURRENT_VERSION {
                match version {
                    0 => v1::migrate(tx)?,
                    other => unreachable!("no migration defined for database version {other}"),
                }
                version += 1;
                tx.db()
                    .cf(DatabaseMigrationVersion)?
                    .put(&ByteColumn, &version, OPERATION)?;
            }
            info!(
                target: LOG_TARGET,
                "🔀 Database migrated to version {CURRENT_VERSION} in {:.2?}",
                timer.elapsed()
            );
        },
        // A fresh database: lay down the genesis state and stamp the current version.
        None => {
            info!(target: LOG_TARGET, "🌱 Fresh database - adding genesis state");
            create_genesis_state(tx, network, consensus_constants.num_preshards)?;
            tx.db()
                .cf(DatabaseMigrationVersion)?
                .put(&ByteColumn, &CURRENT_VERSION, OPERATION)?;
        },
    }

    Ok(())
}
