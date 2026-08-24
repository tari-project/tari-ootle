//   Copyright 2022 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Debug, fs::create_dir_all, path::PathBuf, time::Duration};

use async_trait::async_trait;
use deadpool_diesel::{
    Runtime,
    sqlite::{Hook, HookError, Manager, Pool},
};
use diesel::{Connection, RunQueryDsl, SqliteConnection, sql_query};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness};
use tari_ootle_storage::StorageError;
use tari_ootle_storage_sqlite::{SqliteTransaction, error::SqliteStorageError};

use crate::{
    storage_sqlite::{reader::SqliteStoreReadTransaction, writer::SqliteStoreWriteTransaction},
    store::{IndexerStore, IndexerStoreReader, IndexerStoreWriteTransaction},
};

const LOG_TARGET: &str = "tari::indexer::storage_sqlite";
const POOL_MAX_SIZE: usize = 16;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct SqliteIndexerStore {
    pool: Pool,
}

impl SqliteIndexerStore {
    pub fn try_create(path: PathBuf) -> Result<Self, StorageError> {
        create_dir_all(path.parent().unwrap()).map_err(|_| StorageError::FileSystemPathDoesNotExist)?;

        let database_url = path.to_str().expect("database_url utf-8 error").to_string();

        // Run migrations on a one-shot connection before opening the pool, so pooled connections
        // never observe a partially-migrated schema.
        let mut migration_conn = SqliteConnection::establish(&database_url).map_err(SqliteStorageError::from)?;
        apply_pragmas(&mut migration_conn).map_err(|source| SqliteStorageError::DieselError {
            source,
            operation: "set pragma",
        })?;
        pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./src/storage_sqlite/migrations");
        if let Err(err) = migration_conn.run_pending_migrations(MIGRATIONS) {
            log::error!(target: LOG_TARGET, "Error running migrations: {}", err);
        }
        drop(migration_conn);

        let manager = Manager::new(database_url, Runtime::Tokio1);
        let pool = Pool::builder(manager)
            .max_size(POOL_MAX_SIZE)
            .post_create(Hook::async_fn(|conn, _metrics| {
                Box::pin(async move {
                    conn.interact(apply_pragmas)
                        .await
                        .map_err(|e| HookError::message(format!("post_create panicked: {e}")))?
                        .map_err(|e| HookError::message(format!("apply_pragmas failed: {e}")))?;
                    Ok(())
                })
            }))
            .build()
            .map_err(|e| StorageError::General {
                details: format!("Failed to build sqlite connection pool: {}", e),
            })?;

        Ok(Self { pool })
    }

    async fn acquire(&self) -> Result<deadpool_diesel::sqlite::Connection, StorageError> {
        self.pool.get().await.map_err(|e| StorageError::General {
            details: format!("Failed to acquire sqlite connection from pool: {}", e),
        })
    }
}

impl Debug for SqliteIndexerStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SqliteIndexerStore {{ pool: ... }}")
    }
}

#[async_trait]
impl IndexerStoreReader for SqliteIndexerStore {
    type ReadTransaction<'a> = SqliteStoreReadTransaction<'a>;

    async fn with_read_tx<F, R, E>(&self, f: F) -> Result<R, E>
    where
        F: for<'a> FnOnce(&mut Self::ReadTransaction<'a>) -> Result<R, E> + Send + 'static,
        R: Send + 'static,
        E: From<StorageError> + Send + 'static,
    {
        let conn = self.acquire().await?;
        let result: Result<R, E> = conn
            .interact(move |c| -> Result<R, E> {
                let inner = SqliteTransaction::begin(c)
                    .map_err(StorageError::from)
                    .map_err(E::from)?;
                let mut tx = SqliteStoreReadTransaction::new(inner);
                f(&mut tx)
            })
            .await
            .map_err(|e| StorageError::General {
                details: format!("Pool interact panicked: {}", e),
            })?;
        result
    }
}

#[async_trait]
impl IndexerStore for SqliteIndexerStore {
    type WriteTransaction<'a> = SqliteStoreWriteTransaction<'a>;

    async fn with_write_tx<F, R, E>(&self, f: F) -> Result<R, E>
    where
        F: for<'a> FnOnce(&mut Self::WriteTransaction<'a>) -> Result<R, E> + Send + 'static,
        R: Send + 'static,
        E: From<StorageError> + Send + 'static,
    {
        let conn = self.acquire().await?;
        let result: Result<R, E> = conn
            .interact(move |c| -> Result<R, E> {
                let inner = SqliteTransaction::begin_immediate(c)
                    .map_err(StorageError::from)
                    .map_err(E::from)?;
                let mut tx = SqliteStoreWriteTransaction::new(inner);
                match f(&mut tx) {
                    Ok(r) => {
                        tx.commit().map_err(E::from)?;
                        Ok(r)
                    },
                    Err(e) => {
                        if let Err(err) = tx.rollback() {
                            log::error!(target: LOG_TARGET, "Failed to rollback transaction: {}", err);
                        }
                        Err(e)
                    },
                }
            })
            .await
            .map_err(|e| StorageError::General {
                details: format!("Pool interact panicked: {}", e),
            })?;
        result
    }
}

fn apply_pragmas(conn: &mut SqliteConnection) -> Result<(), diesel::result::Error> {
    let busy_timeout_ms = BUSY_TIMEOUT.as_millis();
    sql_query("PRAGMA journal_mode = WAL;").execute(conn)?;
    sql_query("PRAGMA synchronous = NORMAL;").execute(conn)?;
    sql_query("PRAGMA foreign_keys = ON;").execute(conn)?;
    sql_query(format!("PRAGMA busy_timeout = {};", busy_timeout_ms)).execute(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tari_common_types::types::FixedHash;
    use tari_engine_types::{
        fees::FeeReceiptBuilder,
        transaction_receipt::{FinalizeOutcome, TransactionReceipt},
    };
    use tari_ootle_common_types::{Epoch, NodeHeight, ShardGroup};
    use tari_ootle_transaction::{Transaction, TransactionId};

    use super::*;
    use crate::{
        storage_sqlite::models::VerifiedStateRoot,
        store::{IndexerStoreReadTransaction, IndexerStoreReader, IndexerStoreWriteTransaction},
    };

    fn shard_group() -> ShardGroup {
        ShardGroup::new_checked(1, 4).unwrap()
    }

    fn tip_at(height: u64) -> VerifiedStateRoot {
        // Each height gets a distinct root, as committed heights do on a real chain.
        VerifiedStateRoot {
            epoch: Epoch(1),
            shard_group: shard_group(),
            height: NodeHeight(height),
            block_hash: FixedHash::new([height as u8; 32]),
            state_merkle_root: FixedHash::new([height as u8; 32]),
        }
    }

    async fn temp_store() -> (tempfile::TempDir, SqliteIndexerStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteIndexerStore::try_create(dir.path().join("indexer.db")).unwrap();
        (dir, store)
    }

    #[tokio::test]
    async fn tari_economics_round_trips() {
        use tari_template_lib_types::Amount;

        use crate::{
            storage_sqlite::models::Key,
            store::{IndexerStoreWriteTransaction, ReadOnlyStore},
        };

        let (_dir, store) = temp_store().await;
        store
            .with_write_tx(|tx| {
                tx.key_value_set(Key::TariAccumulatedClaimed, Amount::from(1_000u64))?;
                tx.key_value_set(Key::TariAccumulatedExhaustBurn, Amount::from(50u64))?;
                tx.key_value_set(Key::TariAccumulatedFees, Amount::from(800u64))?;
                tx.key_value_set(Key::TariAccumulatedReceiptExhaustBurn, Amount::from(40u64))
            })
            .await
            .unwrap();

        let econ = ReadOnlyStore::new(store.clone()).get_tari_economics().await.unwrap();
        assert_eq!(econ.total_claimed, Amount::from(1_000u64));
        assert_eq!(econ.total_exhaust_burned, Amount::from(50u64));
        assert_eq!(econ.fee_volume, Amount::from(800u64));
        assert_eq!(econ.receipt_exhaust_burned, Amount::from(40u64));
    }

    #[tokio::test]
    async fn tari_economics_defaults_to_zero_when_unset() {
        use tari_template_lib_types::Amount;

        use crate::store::ReadOnlyStore;

        let (_dir, store) = temp_store().await;
        let econ = ReadOnlyStore::new(store.clone()).get_tari_economics().await.unwrap();
        assert_eq!(econ.total_claimed, Amount::zero());
        assert_eq!(econ.fee_volume, Amount::zero());
        assert_eq!(econ.receipt_exhaust_burned, Amount::zero());
    }

    #[tokio::test]
    async fn tari_total_supply_nets_receipt_burn_not_header() {
        use tari_template_lib_types::Amount;

        use crate::{
            storage_sqlite::models::Key,
            store::{IndexerStoreWriteTransaction, ReadOnlyStore},
        };

        let (_dir, store) = temp_store().await;
        store
            .with_write_tx(|tx| {
                tx.key_value_set(Key::TariAccumulatedClaimed, Amount::from(1_000u64))?;
                // Deliberately different from the receipt burn to prove supply ignores the header total.
                tx.key_value_set(Key::TariAccumulatedExhaustBurn, Amount::from(999u64))?;
                tx.key_value_set(Key::TariAccumulatedReceiptExhaustBurn, Amount::from(40u64))
            })
            .await
            .unwrap();

        let supply = ReadOnlyStore::new(store.clone()).get_tari_total_supply().await.unwrap();
        assert_eq!(supply, Amount::from(960u64));
    }

    #[tokio::test]
    async fn verified_state_roots_ring_prunes_to_sixteen() {
        let (_dir, store) = temp_store().await;

        // Record 19 distinct committed heights for the same (epoch, shard group).
        for h in 1..=19u64 {
            let root = tip_at(h);
            store
                .with_write_tx(move |tx| tx.upsert_verified_state_root(&root))
                .await
                .unwrap();
        }

        // The latest reflects the most recent committed height.
        let latest = store
            .with_read_tx(move |tx| tx.get_latest_verified_state_root(Epoch(1), shard_group()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest.height, NodeHeight(19));

        // The newest 16 (heights 4..=19) remain trusted; the oldest 3 (1..=3) were pruned.
        for h in 4..=19u64 {
            let root = FixedHash::new([h as u8; 32]);
            assert!(
                store
                    .with_read_tx(move |tx| tx.is_state_root_trusted(Epoch(1), shard_group(), &root))
                    .await
                    .unwrap(),
                "height {h} should still be trusted"
            );
        }
        for h in 1..=3u64 {
            let root = FixedHash::new([h as u8; 32]);
            assert!(
                !store
                    .with_read_tx(move |tx| tx.is_state_root_trusted(Epoch(1), shard_group(), &root))
                    .await
                    .unwrap(),
                "height {h} should have been pruned"
            );
        }
    }

    #[tokio::test]
    async fn verified_state_roots_upsert_is_idempotent() {
        let (_dir, store) = temp_store().await;
        for _ in 0..3 {
            let root = tip_at(5);
            store
                .with_write_tx(move |tx| tx.upsert_verified_state_root(&root))
                .await
                .unwrap();
        }
        let hash = FixedHash::new([5u8; 32]);
        assert!(
            store
                .with_read_tx(move |tx| tx.is_state_root_trusted(Epoch(1), shard_group(), &hash))
                .await
                .unwrap()
        );
        // An unrecorded root is not trusted.
        let other = FixedHash::new([99u8; 32]);
        assert!(
            !store
                .with_read_tx(move |tx| tx.is_state_root_trusted(Epoch(1), shard_group(), &other))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn recent_transactions_include_receipt_summary() {
        use tari_common_types::types::PrivateKey;
        use tari_engine_types::{
            fees::FeeReceiptBuilder,
            transaction_receipt::{FinalizeOutcome, TransactionReceipt},
        };
        use tari_ootle_transaction::Transaction;

        use crate::store::IndexerStoreWriteTransaction;

        let (_dir, store) = temp_store().await;

        let transaction = Transaction::builder_localnet(Epoch(1)).build_and_seal(&PrivateKey::from(123u64));
        let tx_id = transaction.calculate_id();
        store
            .with_write_tx(move |tx| tx.insert_or_ignore_transaction(&transaction))
            .await
            .unwrap();

        // No receipt indexed yet — the transaction lists without a summary.
        let entries = store
            .with_read_tx(move |tx| tx.list_recent_transactions(None, 10))
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].transaction_id, tx_id);
        assert!(entries[0].summary.is_none());

        let receipt = TransactionReceipt {
            outcome: FinalizeOutcome::FeeIntentCommit,
            diff_summary: Default::default(),
            fee_withdrawals: [].into(),
            events: [].into(),
            fee_receipt: FeeReceiptBuilder::default().with_total_fees_paid(123).build(),
            epoch: Epoch(1),
            intent_commitment: Default::default(),
        };
        store
            .with_write_tx(move |tx| {
                tx.batch_insert_transaction_receipts([(tx_id.into_receipt_address(), receipt)], &[])
            })
            .await
            .unwrap();

        let entries = store
            .with_read_tx(move |tx| tx.list_recent_transactions(None, 10))
            .await
            .unwrap();
        let summary = entries[0].summary.as_ref().unwrap();
        assert!(summary.outcome.is_fee_intent_commit());
        assert_eq!(summary.total_fees_paid, 123);

        let entry = store
            .with_read_tx(move |tx| tx.get_transaction(tx_id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.summary.as_ref().unwrap().total_fees_paid, 123);
    }

    #[tokio::test]
    async fn prune_transactions_removes_only_aged_rows_and_keeps_receipts() {
        let (_dir, store) = temp_store().await;

        // A transaction's retention epoch is its max_epoch until a receipt supplies a commit epoch.
        let ids = insert_transactions(&store, &[Epoch(5), Epoch(6), Epoch(20)]).await;

        // A receipt for a transaction that is about to be pruned: it must survive.
        let receipt_address = ids[0].into_receipt_address();
        store
            .with_write_tx(move |tx| {
                tx.batch_insert_transaction_receipts([(receipt_address, receipt_at(Epoch(5)))], &[])
            })
            .await
            .unwrap();

        let num_pruned = store
            .with_write_tx(move |tx| tx.prune_transactions_before_epoch(Epoch(10), 100))
            .await
            .unwrap();
        assert_eq!(num_pruned, 2);

        let remaining = store
            .with_read_tx(move |tx| tx.list_recent_transactions(None, 10))
            .await
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].transaction_id, ids[2]);

        // The receipt of the pruned transaction is untouched.
        store
            .with_read_tx(move |tx| tx.get_transaction_receipt(&receipt_address))
            .await
            .unwrap();
    }

    /// A transaction that never commits gets no receipt — mempool rejection and a consensus abort that
    /// commits nothing both leave one — so its `max_epoch` stays its retention key. Without that
    /// fallback these rows, the ones retention exists to bound, would never age out.
    #[tokio::test]
    async fn a_transaction_that_never_commits_is_retained_on_its_max_epoch() {
        let (_dir, store) = temp_store().await;

        let ids = insert_transactions(&store, &[Epoch(5), Epoch(20)]).await;
        for id in &ids {
            let id = *id;
            store
                .with_write_tx(move |tx| tx.set_transaction_rejected(id, "rejected by mempool validation"))
                .await
                .unwrap();
        }

        let num_pruned = store
            .with_write_tx(move |tx| tx.prune_transactions_before_epoch(Epoch(10), 100))
            .await
            .unwrap();
        assert_eq!(num_pruned, 1);

        let remaining = store
            .with_read_tx(move |tx| tx.list_recent_transactions(None, 10))
            .await
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].transaction_id, ids[1]);
    }

    /// A committed transaction is retained from the epoch it committed in, not from the last epoch it
    /// could have been sequenced in — a wide `max_epoch` must not hold its record open.
    #[tokio::test]
    async fn indexing_a_receipt_moves_retention_to_the_commit_epoch() {
        let (_dir, store) = temp_store().await;

        let ids = insert_transactions(&store, &[Epoch(100)]).await;
        let receipt_address = ids[0].into_receipt_address();

        // On its max_epoch alone this transaction is far from the cutoff.
        let num_pruned = store
            .with_write_tx(move |tx| tx.prune_transactions_before_epoch(Epoch(10), 100))
            .await
            .unwrap();
        assert_eq!(num_pruned, 0);

        store
            .with_write_tx(move |tx| {
                tx.batch_insert_transaction_receipts([(receipt_address, receipt_at(Epoch(2)))], &[])
            })
            .await
            .unwrap();

        let num_pruned = store
            .with_write_tx(move |tx| tx.prune_transactions_before_epoch(Epoch(10), 100))
            .await
            .unwrap();
        assert_eq!(num_pruned, 1);
    }

    /// The prune select must be servable from `transactions_retention_epoch_idx`. Ordering it by any
    /// column other than the one it filters on silently degrades it to a full table scan that runs
    /// under the database-wide write lock, including on the common call that prunes nothing.
    #[tokio::test]
    async fn prune_select_is_served_by_the_retention_epoch_index() {
        #[derive(diesel::QueryableByName)]
        struct QueryPlanRow {
            #[diesel(sql_type = diesel::sql_types::Text)]
            detail: String,
        }

        let (_dir, store) = temp_store().await;
        let plan = store
            .with_read_tx(|tx| {
                sql_query(
                    "explain query plan select id from transactions where retention_epoch < 100 order by \
                     retention_epoch asc limit 500",
                )
                .load::<QueryPlanRow>(tx.connection())
                .map_err(|e| StorageError::general("explain query plan", e))
            })
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.detail)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            plan.contains("transactions_retention_epoch_idx"),
            "prune select does not use the retention epoch index: {plan}"
        );
        assert!(
            !plan.contains("SCAN transactions"),
            "prune select falls back to a scan: {plan}"
        );
    }

    #[tokio::test]
    async fn rejection_status_distinguishes_a_pruned_row_from_an_unrejected_one() {
        use tari_common_types::types::PrivateKey;
        use tari_ootle_transaction::Transaction;

        use crate::store::{IndexerStoreWriteTransaction, TransactionRejectionStatus};

        let (_dir, store) = temp_store().await;

        let transaction = Transaction::builder_localnet(Epoch(1)).build_and_seal(&PrivateKey::from(7u64));
        let tx_id = transaction.calculate_id();

        // Never submitted here.
        let status = store
            .with_read_tx(move |tx| tx.get_transaction_rejection_status(tx_id))
            .await
            .unwrap();
        assert!(matches!(status, TransactionRejectionStatus::NotStored));

        store
            .with_write_tx(move |tx| tx.insert_or_ignore_transaction(&transaction))
            .await
            .unwrap();
        let status = store
            .with_read_tx(move |tx| tx.get_transaction_rejection_status(tx_id))
            .await
            .unwrap();
        assert!(matches!(status, TransactionRejectionStatus::NotRejected));

        store
            .with_write_tx(move |tx| tx.set_transaction_rejected(tx_id, "nope"))
            .await
            .unwrap();
        let status = store
            .with_read_tx(move |tx| tx.get_transaction_rejection_status(tx_id))
            .await
            .unwrap();
        assert!(matches!(status, TransactionRejectionStatus::Rejected { details, .. } if details == "nope"));

        // Pruned rows report as unstored, so callers do not re-issue the rejection write forever.
        store
            .with_write_tx(move |tx| tx.prune_transactions_before_epoch(Epoch(10), 100))
            .await
            .unwrap();
        let status = store
            .with_read_tx(move |tx| tx.get_transaction_rejection_status(tx_id))
            .await
            .unwrap();
        assert!(matches!(status, TransactionRejectionStatus::NotStored));
    }

    #[tokio::test]
    async fn recent_transactions_returns_an_empty_page_when_the_cursor_was_pruned() {
        use tari_common_types::types::PrivateKey;
        use tari_ootle_transaction::Transaction;

        use crate::store::IndexerStoreWriteTransaction;

        let (_dir, store) = temp_store().await;

        let transaction = Transaction::builder_localnet(Epoch(1)).build_and_seal(&PrivateKey::from(11u64));
        let cursor = transaction.calculate_id();
        store
            .with_write_tx(move |tx| tx.insert_or_ignore_transaction(&transaction))
            .await
            .unwrap();

        store
            .with_write_tx(move |tx| tx.prune_transactions_before_epoch(Epoch(10), 100))
            .await
            .unwrap();

        let page = store
            .with_read_tx(move |tx| tx.list_recent_transactions(Some(cursor), 10))
            .await
            .unwrap();
        assert!(page.is_empty());
    }
    async fn insert_transactions(store: &SqliteIndexerStore, max_epochs: &[Epoch]) -> Vec<TransactionId> {
        use tari_common_types::types::PrivateKey;

        let mut ids = Vec::new();
        for (i, max_epoch) in max_epochs.iter().enumerate() {
            let transaction = Transaction::builder_localnet(*max_epoch).build_and_seal(&PrivateKey::from(i as u64));
            ids.push(transaction.calculate_id());
            store
                .with_write_tx(move |tx| tx.insert_or_ignore_transaction(&transaction))
                .await
                .unwrap();
        }
        ids
    }

    fn receipt_at(epoch: Epoch) -> TransactionReceipt {
        TransactionReceipt {
            outcome: FinalizeOutcome::FeeIntentCommit,
            diff_summary: Default::default(),
            fee_withdrawals: [].into(),
            events: [].into(),
            fee_receipt: FeeReceiptBuilder::default().with_total_fees_paid(123).build(),
            epoch,
            intent_commitment: Default::default(),
        }
    }
}
