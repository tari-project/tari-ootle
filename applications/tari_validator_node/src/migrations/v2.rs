//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Gives the transaction pool state update debug history its table prefix.
//!
//! The table shares the diagnostics column family with the no-vote diagnostics, where each table is namespaced by a
//! leading [`KeyPrefix`] byte. It declared a prefix but never encoded it, so its keys sat in the low keyspace of that
//! family, led by a big-endian `Epoch`. This migration rewrites each of those keys with the prefix the table now
//! encodes.
//!
//! Nothing collided in practice - an `Epoch` only reaches the no-vote prefix byte at absurd epoch numbers - so this
//! removes latent fragility rather than fixing observed corruption. The table is only written when `debugging_data` is
//! enabled, so on most databases there is nothing to rewrite.

use std::time::Instant;

use log::*;
use tari_ootle_common_types::NodeAddressable;
use tari_state_store_rocksdb::{
    codecs::KeyPrefix,
    column_families::transaction_pool_state_update,
    writer::RocksDbStateStoreWriteTransaction,
};

use super::common::rewrite_unprefixed_rows;

const LOG_TARGET: &str = "tari::validator::migrations::v2";

pub fn migrate<TAddr: NodeAddressable + 'static>(
    tx: &mut RocksDbStateStoreWriteTransaction<'_, TAddr>,
) -> anyhow::Result<()> {
    const OPERATION: &str = "migrations::v2";
    let timer = Instant::now();

    // The only other table in the diagnostics family is prefixed `DiagnosticsNoVotes`.
    let num_migrated = rewrite_unprefixed_rows(
        &tx.db()
            .cf(transaction_pool_state_update::LegacyUnprefixedDebugHistoryCf)?,
        &tx.db()
            .cf(transaction_pool_state_update::TransactionPoolStateUpdateDebugHistoryCf)?,
        KeyPrefix::DiagnosticsNoVotes.as_u8(),
        OPERATION,
    )?;

    if num_migrated == 0 {
        debug!(target: LOG_TARGET, "No unprefixed debug history entries to migrate");
        return Ok(());
    }

    info!(
        target: LOG_TARGET,
        "🔀 Prefixed {num_migrated} debug history entries in {:.2?}",
        timer.elapsed()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use tari_common_types::types::FixedHash;
    use tari_consensus_types::BlockId;
    use tari_ootle_storage::StateStore;
    use tari_state_store_rocksdb::column_families::diagnostic_no_vote::{DiagnosticsNoVoteCf, DiagnosticsNoVoteData};

    use super::*;
    use crate::migrations::test_helpers::open_store;

    /// The debug history shares the diagnostics column family with the no-vote table, whose rows lead with a byte
    /// above the scan bound and so must survive untouched.
    #[test]
    fn it_leaves_no_vote_diagnostics_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let store = open_store(&tmp);

        let block_id = BlockId::new(FixedHash::from([2u8; FixedHash::byte_size()]));
        store
            .with_write_tx(|tx| {
                tx.db().cf(DiagnosticsNoVoteCf)?.put(
                    &block_id,
                    &DiagnosticsNoVoteData {
                        reason: "because".into(),
                    },
                    "test",
                )?;
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();

        store.with_write_tx(migrate).unwrap();

        store
            .with_write_tx(|tx| {
                let data = tx.db().cf(DiagnosticsNoVoteCf)?.get(&block_id, "test")?;
                assert_eq!(&*data.reason, "because");
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }
}
