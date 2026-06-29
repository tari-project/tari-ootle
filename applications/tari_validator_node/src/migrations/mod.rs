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
//! There are currently no migrations: the previous ones were folded into the genesis state on a
//! testnet reset (see [`CURRENT_VERSION`]).
//!
//! # Adding a migration
//!
//! To take the schema from version 0 to 1:
//!
//! 1. Add `v1.rs` with `pub fn migrate(...) -> ...` performing the upgrade, and declare it here with `mod v1;`.
//! 2. Bump [`CURRENT_VERSION`] to `1`.
//! 3. Apply it to already-bootstrapped databases by stepping the stored version up to `CURRENT_VERSION`, persisting the
//!    new version as you go - replace the `Some(version)` arm in [`migrate`] with:
//!
//! ```ignore
//! Some(mut version) => {
//!     while version < CURRENT_VERSION {
//!         match version {
//!             0 => v1::migrate(tx, network, consensus_constants.num_preshards)?,
//!             other => unreachable!("no migration defined for database version {other}"),
//!         }
//!         version += 1;
//!         tx.db().cf(DatabaseMigrationVersion)?.put(&ByteColumn, &version, OPERATION)?;
//!     }
//! },
//! ```
//!
//! IMPORTANT: a migration that creates or mutates substates must write them to the per-shard state
//! tree (JMT), not only the substate store - otherwise they have no inclusion proof and verified
//! reads of them fail. Mirror [`crate::genesis_state::create_genesis_state`], which commits each
//! substate to both the store and the state tree. (Note that, unlike genesis, adding state-tree
//! entries to a live chain shifts its state root, so such a migration is itself consensus-affecting.)

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
/// already-running nodes. It was reset to 0 with the genesis-in-state-tree testnet reset: the former
/// v1 (token symbol) and v2 (faucet claim resource) migrations are now part of the genesis state, so
/// they no longer need to run.
const CURRENT_VERSION: u64 = 0;

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
        // An already-bootstrapped database. No migrations are currently defined; when one is needed,
        // step `version` up to `CURRENT_VERSION` here, applying each upgrade and persisting the new
        // version as it goes.
        Some(version) => {
            debug!(
                target: LOG_TARGET,
                "Database already bootstrapped at migration version {version} (current {CURRENT_VERSION})"
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
