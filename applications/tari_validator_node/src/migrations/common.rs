//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Helpers shared by more than one migration.

use tari_ootle_storage::{Ordering, StorageError};
use tari_state_store_rocksdb::{
    cf_api::CfContext,
    traits::{Cf, RocksReader, RocksWriter},
};

/// Rewrites every row below `upper_bound` in the column family shared by `legacy_cf` and `current_cf`, moving it from
/// the legacy unprefixed key encoding to the prefixed one. Returns the number of rows rewritten.
///
/// Tables sharing a column family are namespaced by a leading `KeyPrefix` byte. A table that declared a prefix but
/// never encoded it wrote its keys into the low keyspace of that family instead, below every prefix in use.
///
/// `upper_bound` must be the lowest table prefix in use in that column family, so that everything below it is a legacy
/// row. Each scan is confined to a single leading byte, which keeps it consistent with the family's one byte prefix
/// extractor, and a rewritten key leads with a byte at or above the bound, so it lands outside every scanned range and
/// is never revisited. That makes the rewrite idempotent and safe to interrupt.
pub fn rewrite_unprefixed_rows<DB, TLegacy, TCurrent>(
    legacy_cf: &CfContext<'_, DB, TLegacy>,
    current_cf: &CfContext<'_, DB, TCurrent>,
    upper_bound: u8,
    operation: &'static str,
) -> Result<usize, StorageError>
where
    DB: RocksReader + RocksWriter,
    TLegacy: Cf,
    TCurrent: Cf<Key = TLegacy::Key, Value = TLegacy::Value>,
{
    let mut num_rewritten = 0usize;
    for leading_byte in 0..upper_bound {
        for result in legacy_cf.prefix_range_iterator_raw_key(Ordering::Ascending, vec![leading_byte]) {
            let (key, value) = result?;
            // The same key encodes without the prefix through the legacy table and with it through the current one.
            legacy_cf.delete(&key, operation)?;
            current_cf.put(&key, &value, operation)?;
            num_rewritten += 1;
        }
    }

    Ok(num_rewritten)
}
