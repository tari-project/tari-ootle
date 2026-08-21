//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Gives the substate lock substate-id index its table prefix.
//!
//! The index shares a column family with the substate tables, where each table is namespaced by a leading
//! [`KeyPrefix`] byte. This index was written without one, so its keys sat in the low keyspace of that family,
//! namespaced only by the leading borsh `SubstateId` discriminant. This migration rewrites each of those keys with
//! the prefix the index now encodes.

use std::time::Instant;

use log::*;
use tari_ootle_common_types::NodeAddressable;
use tari_state_store_rocksdb::{
    codecs::KeyPrefix,
    column_families::substate_locks,
    writer::RocksDbStateStoreWriteTransaction,
};

use super::common::rewrite_unprefixed_rows;

const LOG_TARGET: &str = "tari::validator::migrations::v1";

pub fn migrate<TAddr: NodeAddressable + 'static>(
    tx: &mut RocksDbStateStoreWriteTransaction<'_, TAddr>,
) -> anyhow::Result<()> {
    const OPERATION: &str = "migrations::v1";
    let timer = Instant::now();

    // Every other table in the substates family is prefixed at or above `ForeignSubstatePledges`.
    let num_migrated = rewrite_unprefixed_rows(
        &tx.db().cf(substate_locks::LegacyUnprefixedSubstateIdIndex)?,
        &tx.db().cf(substate_locks::SubstateIdIndex)?,
        KeyPrefix::ForeignSubstatePledges.as_u8(),
        OPERATION,
    )?;

    if num_migrated == 0 {
        debug!(target: LOG_TARGET, "No unprefixed substate lock index entries to migrate");
        return Ok(());
    }

    info!(
        target: LOG_TARGET,
        "🔀 Prefixed {num_migrated} substate lock index entries in {:.2?}",
        timer.elapsed()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use tari_common_types::types::FixedHash;
    use tari_consensus_types::BlockId;
    use tari_ootle_common_types::{NodeHeight, SubstateLockType};
    use tari_ootle_storage::StateStore;
    use tari_ootle_transaction::TransactionId;
    use tari_template_lib::types::{ComponentAddress, ObjectKey};

    use super::*;
    use crate::migrations::test_helpers::open_store;

    fn lock_key(seed: u8) -> substate_locks::SubstateLockKey {
        substate_locks::SubstateLockKey {
            block_id: BlockId::new(FixedHash::from([seed; FixedHash::byte_size()])),
            block_height: NodeHeight(u64::from(seed)),
            substate_id: ComponentAddress::from_array([seed; ObjectKey::LENGTH]).into(),
            transaction_id: TransactionId::new([seed; 32]),
        }
    }

    /// Deleting each entry as the scan yields it must not cause any to be skipped.
    #[test]
    fn it_migrates_every_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let store = open_store(&tmp);

        let keys = (0u8..=255).map(lock_key).collect::<Vec<_>>();
        store
            .with_write_tx(|tx| {
                let legacy_cf = tx.db().cf(substate_locks::LegacyUnprefixedSubstateIdIndex)?;
                for key in &keys {
                    legacy_cf.put(key, &SubstateLockType::Write, "test")?;
                }
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();

        store.with_write_tx(migrate).unwrap();

        store
            .with_write_tx(|tx| {
                let legacy_cf = tx.db().cf(substate_locks::LegacyUnprefixedSubstateIdIndex)?;
                let new_cf = tx.db().cf(substate_locks::SubstateIdIndex)?;
                for key in &keys {
                    assert_eq!(new_cf.get(key, "test")?, SubstateLockType::Write, "missing {key}");
                    assert!(!legacy_cf.exists(key, "test")?, "legacy entry {key} was not removed");
                }
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    #[test]
    fn it_prefixes_legacy_entries_and_leaves_the_rest_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let store = open_store(&tmp);

        let legacy_keys = (1u8..=3).map(lock_key).collect::<Vec<_>>();
        // An entry already carrying the prefix, which the migration must neither rescan nor prefix again.
        let already_prefixed = lock_key(4);

        store
            .with_write_tx(|tx| {
                let legacy_cf = tx.db().cf(substate_locks::LegacyUnprefixedSubstateIdIndex)?;
                for key in &legacy_keys {
                    legacy_cf.put(key, &SubstateLockType::Write, "test")?;
                }
                tx.db()
                    .cf(substate_locks::SubstateIdIndex)?
                    .put(&already_prefixed, &SubstateLockType::Read, "test")?;
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();

        store.with_write_tx(migrate).unwrap();

        store
            .with_write_tx(|tx| {
                let legacy_cf = tx.db().cf(substate_locks::LegacyUnprefixedSubstateIdIndex)?;
                let new_cf = tx.db().cf(substate_locks::SubstateIdIndex)?;

                for key in &legacy_keys {
                    assert!(!legacy_cf.exists(key, "test")?, "legacy entry {key} was not removed");
                    assert_eq!(new_cf.get(key, "test")?, SubstateLockType::Write);
                }

                // Untouched, and still readable exactly once under the prefix.
                assert_eq!(new_cf.get(&already_prefixed, "test")?, SubstateLockType::Read);
                assert!(!legacy_cf.exists(&already_prefixed, "test")?);

                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    #[test]
    fn it_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let store = open_store(&tmp);

        let key = lock_key(7);
        store
            .with_write_tx(|tx| {
                tx.db().cf(substate_locks::LegacyUnprefixedSubstateIdIndex)?.put(
                    &key,
                    &SubstateLockType::Output,
                    "test",
                )?;
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();

        store.with_write_tx(migrate).unwrap();
        // A second run has nothing left below the bound to rewrite, so the entry keeps its single prefix.
        store.with_write_tx(migrate).unwrap();

        store
            .with_write_tx(|tx| {
                assert_eq!(
                    tx.db().cf(substate_locks::SubstateIdIndex)?.get(&key, "test")?,
                    SubstateLockType::Output
                );
                assert!(
                    !tx.db()
                        .cf(substate_locks::LegacyUnprefixedSubstateIdIndex)?
                        .exists(&key, "test")?
                );
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }
}
