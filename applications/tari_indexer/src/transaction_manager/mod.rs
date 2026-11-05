//  Copyright 2023, The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

pub(crate) mod error;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::anyhow;
use indexmap::IndexSet;
use log::*;
use tari_epoch_manager::EpochManagerReader;
use tari_indexer_client::types::{IndexerTransactionFinalizedResult, TransactionEntry, TransactionSource};
use tari_ootle_common_types::{Epoch, NodeAddressable};
use tari_ootle_storage::StorageError;
use tari_ootle_transaction::{Network, Transaction, TransactionId};
use tari_ootle_transaction_validation::{Validator, create_structural_transaction_validator};
use tari_validator_node_rpc::client::{TransactionResultStatus, ValidatorNodeClientFactory};
use tokio::time;

use crate::{
    network_client::TariNetworkClient,
    store::{IndexerStore, IndexerStoreReadTransaction, IndexerStoreWriteTransaction, TransactionRejectionStatus},
    substate_cache::SqliteSubstateCache,
    transaction_manager::error::TransactionManagerError,
};

const LOG_TARGET: &str = "tari::indexer::transaction_manager";

/// How many finalized transactions are remembered as already having had their cache entries
/// retired. Only the first time a result is handed out matters, since that is the only moment a
/// fetch can be in flight from before this indexer knew of the commit; remembering it stops every
/// later poll of the same result from taking the write lock again.
const RETIRED_RESULTS_CAPACITY: usize = 4096;

/// How long a request waits to record a transaction's submission or abort locally before answering.
/// The record serves the recent-transactions listing and rejection reporting, and the request
/// recording it never reads it back.
const LOCAL_RECORD_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub struct TransactionManager<TEpochManager, TClientFactory, TStore> {
    network_client: TariNetworkClient<TEpochManager, TClientFactory>,
    store: TStore,
    network: Network,
    max_transaction_weight: u64,
    max_transaction_size_bytes: usize,
    max_transaction_validity_epochs: u64,
    /// Told of every finalized commit this manager hands out, so that a caller reading what its
    /// transaction created is not answered from before the commit. See
    /// [`SqliteSubstateCache::retire_committed`].
    substate_cache: SqliteSubstateCache,
    /// Finalized transactions whose commit has already been retired from the cache, most recent
    /// last, bounded at [`RETIRED_RESULTS_CAPACITY`].
    retired_results: Arc<Mutex<IndexSet<TransactionId>>>,
}

impl<TEpochManager, TClientFactory, TAddr, TStore> TransactionManager<TEpochManager, TClientFactory, TStore>
where
    TAddr: NodeAddressable + 'static,
    TEpochManager: EpochManagerReader<Addr = TAddr> + 'static,
    TClientFactory: ValidatorNodeClientFactory<TAddr> + 'static,
    TStore: IndexerStore,
{
    pub fn new(
        network_client: TariNetworkClient<TEpochManager, TClientFactory>,
        store: TStore,
        network: Network,
        max_transaction_weight: u64,
        max_transaction_size_bytes: usize,
        max_transaction_validity_epochs: u64,
        substate_cache: SqliteSubstateCache,
    ) -> Self {
        Self {
            network_client,
            store,
            network,
            max_transaction_weight,
            max_transaction_size_bytes,
            max_transaction_validity_epochs,
            substate_cache,
            retired_results: Arc::new(Mutex::new(IndexSet::with_capacity(RETIRED_RESULTS_CAPACITY))),
        }
    }

    fn is_result_retired(&self, transaction_id: &TransactionId) -> bool {
        self.retired_results
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(transaction_id)
    }

    /// Records that `transaction_id`'s commit has been retired from the cache. Recorded only after
    /// the retirement succeeded, so a failed one is tried again on the next poll and concurrent
    /// pollers each wait on their own (idempotent) retirement rather than one racing past the
    /// other's.
    fn mark_result_retired(&self, transaction_id: TransactionId) {
        let mut retired = self.retired_results.lock().unwrap_or_else(|e| e.into_inner());
        retired.insert(transaction_id);
        if retired.len() > RETIRED_RESULTS_CAPACITY {
            retired.shift_remove_index(0);
        }
    }

    pub async fn submit_transaction(&self, transaction: Transaction) -> Result<TransactionId, TransactionManagerError> {
        let transaction_id = transaction.calculate_id();
        // Run the structural mempool validations (network, basic, weight, signature) before
        // forwarding to validator committees, so malformed or over-weight transactions are rejected
        // here instead of being fanned out across the network. Context-dependent checks (epoch range,
        // template existence) are left to the validators, whose epoch/template view is authoritative.
        // DEV note: an invalid signature here is probably a JSON decoding issue
        // (crates/engine_types/src/argument_parser.rs).
        let validator = create_structural_transaction_validator(
            self.network,
            self.max_transaction_weight,
            self.max_transaction_size_bytes,
        );
        if let Err(err) = validator.validate(&(), &transaction) {
            return Err(TransactionManagerError::InvalidTransaction {
                transaction_id,
                details: err.to_string(),
            });
        }
        // The row is recorded whether or not the committee sequences the transaction, so its
        // retention key cannot be taken on trust from the transaction's own `max_epoch`: that would
        // let any request passing the structural checks claim a row the pruner never reaches. Cap it
        // at the last epoch a transaction admitted now could still be sequenced in. `current_epoch`
        // waits out the epoch manager's initial scan, so this cannot cap against the zero the
        // manager reports before then.
        let retention_ceiling = Epoch(
            self.network_client
                .current_epoch()
                .await?
                .as_u64()
                .saturating_add(self.max_transaction_validity_epochs),
        );
        let transaction_for_db = transaction.clone();
        let submitted = self.network_client.submit_transaction(transaction).await;

        // A previously rejected transaction may be accepted on resubmission (e.g. after a missing
        // template is published), so a stale rejection must not shadow it.
        let accepted = submitted.is_ok();
        let rejection = submitted
            .as_ref()
            .err()
            .and_then(|err| err.validation_rejection_details())
            .map(str::to_string);
        let record = self.store.with_write_tx(move |tx| {
            tx.upsert_submitted_transaction(&transaction_for_db, retention_ceiling)?;
            match rejection {
                Some(details) => tx.set_transaction_rejected(transaction_id, &details),
                None if accepted => tx.clear_transaction_rejection(transaction_id),
                None => Ok::<_, StorageError>(()),
            }
        });
        match time::timeout(LOCAL_RECORD_TIMEOUT, record).await {
            Ok(Ok(())) => {},
            Ok(Err(e)) => warn!(target: LOG_TARGET, "Failed to record submitted transaction {transaction_id}: {e}"),
            Err(_) => warn!(
                target: LOG_TARGET,
                "Answered the submission of {transaction_id} without recording it: the write did not complete within {}ms",
                LOCAL_RECORD_TIMEOUT.as_millis()
            ),
        }

        Ok(submitted?)
    }

    pub async fn get_transaction_result(
        &self,
        transaction_id: TransactionId,
    ) -> Result<IndexerTransactionFinalizedResult, TransactionManagerError> {
        // The committee is authoritative for any sequenced transaction. This includes aborts, which
        // commit no substate — and therefore no receipt for the indexer to sync — only a finalized
        // decision. Query it first so the full result (including abort decision and execution
        // details) is returned to callers.
        let network_result = self
            .network_client
            .get_finalized_transaction_result(transaction_id)
            .await;

        match network_result {
            Ok(Some(TransactionResultStatus::Finalized(finalized))) => {
                // Record a terminal abort locally so the recent-transactions listing — which reads
                // only local state — reflects it instead of showing the transaction as pending
                // indefinitely. Committed transactions are surfaced via their synced receipt, so
                // only aborts need recording here.
                if finalized.final_decision.is_abort() {
                    // Record the abort only once. Re-recording it on every read would needlessly
                    // take SQLite's single write lock and contend with other writers under load.
                    // A pruned transaction has no row to annotate, and the write would update
                    // nothing while still taking that lock on every read, so it is skipped too.
                    let status = self
                        .store
                        .with_read_tx(move |tx| tx.get_transaction_rejection_status(transaction_id))
                        .await?;
                    if matches!(status, TransactionRejectionStatus::NotRejected) {
                        let details = finalized
                            .abort_details
                            .clone()
                            .or_else(|| finalized.final_decision.abort_reason().map(|r| r.to_string()))
                            .unwrap_or_else(|| "Transaction aborted".to_string());
                        // A record that does not land is retried on the next read of the result.
                        let record = self
                            .store
                            .with_write_tx(move |tx| tx.set_transaction_rejected(transaction_id, &details));
                        match time::timeout(LOCAL_RECORD_TIMEOUT, record).await {
                            Ok(Ok(())) => {},
                            Ok(Err(e)) => {
                                warn!(target: LOG_TARGET, "Failed to record the abort of {transaction_id}: {e}")
                            },
                            Err(_) => warn!(
                                target: LOG_TARGET,
                                "Answered the result of {transaction_id} without recording its abort: the write did not complete within {}ms",
                                LOCAL_RECORD_TIMEOUT.as_millis()
                            ),
                        }
                    }
                }

                // The committee answered ahead of this indexer's own transition stream. A caller
                // holding this result will read what it committed next, so the cache must not
                // answer that from before the commit. A fee-only accept commits its diff too.
                if finalized.final_decision.is_commit() &&
                    !self.is_result_retired(&transaction_id) &&
                    let Some(execute_result) = finalized
                        .execute_result
                        .as_ref()
                        .map(|r| r.decode())
                        .transpose()
                        .map_err(|e| anyhow!("Agreed execution result of {transaction_id} does not decode: {e}"))? &&
                    let Some(diff) = execute_result.finalize.any_accept()
                {
                    match self.substate_cache.retire_committed(diff).await {
                        Ok(()) => self.mark_result_retired(transaction_id),
                        Err(e) => warn!(
                            target: LOG_TARGET,
                            "Failed to retire cached substates committed by {transaction_id}: {e}"
                        ),
                    }
                }

                Ok(IndexerTransactionFinalizedResult::Finalized {
                    final_decision: finalized.final_decision,
                    execution_result: finalized.execute_result,
                    execution_time: finalized.execution_time,
                    finalized_time: finalized.finalized_time,
                    abort_details: finalized.abort_details,
                })
            },
            // The committee has no agreed finalized result: the transaction is genuinely pending, the
            // committee is unreachable, or it was rejected by mempool validation and never sequenced
            // (in which case the committee reports it as pending forever). Prefer a locally recorded
            // rejection over those outcomes.
            other => {
                let status = self
                    .store
                    .with_read_tx(move |tx| tx.get_transaction_rejection_status(transaction_id))
                    .await?;
                if let TransactionRejectionStatus::Rejected { details, rejected_at } = status {
                    return Ok(IndexerTransactionFinalizedResult::Rejected {
                        details,
                        rejected_time: rejected_at,
                    });
                }

                match other {
                    Ok(Some(TransactionResultStatus::Pending)) => Ok(IndexerTransactionFinalizedResult::Pending),
                    Ok(None) => Err(TransactionManagerError::NotFound {
                        entity: "Transaction result",
                        key: transaction_id.to_string(),
                    }),
                    Err(e) => Err(e.into()),
                    Ok(Some(TransactionResultStatus::Finalized(_))) => {
                        unreachable!("Finalized result is handled above")
                    },
                }
            },
        }
    }

    pub async fn list_recent_transactions(
        &self,
        last_id: Option<TransactionId>,
        limit: usize,
        source: Option<TransactionSource>,
    ) -> Result<Vec<TransactionEntry>, TransactionManagerError> {
        let transactions = self
            .store
            .with_read_tx(move |tx| tx.list_recent_transactions(last_id, limit, source))
            .await?;
        Ok(transactions)
    }

    /// Fetch a single transaction (with its instructions) by ID. Returns `None` if the transaction
    /// was not submitted through this indexer.
    pub async fn get_transaction(
        &self,
        transaction_id: TransactionId,
    ) -> Result<Option<TransactionEntry>, TransactionManagerError> {
        let transaction = self
            .store
            .with_read_tx(move |tx| tx.get_transaction(transaction_id))
            .await?;
        Ok(transaction)
    }
}

#[cfg(test)]
mod tests {
    use tari_common_types::types::PrivateKey;
    use tari_template_lib_types::{Amount, ComponentAddress};

    use super::*;
    use crate::{
        network_client::tests::{Answer, FakeCommittee, FakeEpochManager, aborted, network},
        network_state_sync::ShardWatermarks,
        storage_sqlite::{SqliteIndexerStore, temp_store},
        store::IndexerStoreReader,
    };

    fn manager(store: SqliteIndexerStore) -> TransactionManager<FakeEpochManager, FakeCommittee, SqliteIndexerStore> {
        manager_answering(store, Answer::Pending)
    }

    fn manager_answering(
        store: SqliteIndexerStore,
        answer: Answer,
    ) -> TransactionManager<FakeEpochManager, FakeCommittee, SqliteIndexerStore> {
        let (network_client, _) = network(vec![answer]);
        let substate_cache = SqliteSubstateCache::new(
            store.clone(),
            Arc::new(ShardWatermarks::new()),
            Duration::from_secs(60),
            Duration::from_secs(60),
            Duration::from_secs(300),
            Duration::from_secs(900),
            1000,
        );
        TransactionManager::new(
            network_client,
            store,
            Network::LocalNet,
            u64::MAX,
            usize::MAX,
            10,
            substate_cache,
        )
    }

    fn transaction(seed: u64) -> Transaction {
        Transaction::builder_localnet(Epoch(1))
            .pay_fee_from_component(ComponentAddress::from_array([1u8; 32]), Amount::new(1000))
            .build_and_seal(&PrivateKey::from(seed))
    }

    #[tokio::test(start_paused = true)]
    async fn a_submission_is_answered_while_the_store_cannot_write() {
        let (_dir, store) = temp_store().await;
        let manager = manager(store.clone());
        let transaction = transaction(5);
        let _held = store.hold_write_lock().await;

        let started = time::Instant::now();
        let id = manager.submit_transaction(transaction.clone()).await.unwrap();

        assert_eq!(id, transaction.calculate_id());
        assert_eq!(started.elapsed(), LOCAL_RECORD_TIMEOUT);
    }

    #[tokio::test(start_paused = true)]
    async fn an_abort_is_answered_while_the_store_cannot_write() {
        let (_dir, store) = temp_store().await;
        let manager = manager_answering(store.clone(), aborted());
        let transaction = transaction(7);
        let id = transaction.calculate_id();
        store
            .with_write_tx(move |tx| tx.upsert_submitted_transaction(&transaction, Epoch(100)))
            .await
            .unwrap();
        let _held = store.hold_write_lock().await;

        let started = time::Instant::now();
        let result = manager.get_transaction_result(id).await.unwrap();

        assert!(
            matches!(&result, IndexerTransactionFinalizedResult::Finalized { final_decision, .. } if final_decision.is_abort())
        );
        assert_eq!(started.elapsed(), LOCAL_RECORD_TIMEOUT);
    }

    #[tokio::test]
    async fn an_accepted_resubmission_is_recorded_and_clears_its_rejection() {
        let (_dir, store) = temp_store().await;
        let manager = manager(store.clone());
        let transaction = transaction(6);
        let id = transaction.calculate_id();
        let stored = transaction.clone();
        store
            .with_write_tx(move |tx| {
                tx.upsert_submitted_transaction(&stored, Epoch(100))?;
                tx.set_transaction_rejected(id, "template not found")
            })
            .await
            .unwrap();

        manager.submit_transaction(transaction).await.unwrap();

        let (entry, status) = store
            .with_read_tx(move |tx| {
                Ok::<_, StorageError>((tx.get_transaction(id)?, tx.get_transaction_rejection_status(id)?))
            })
            .await
            .unwrap();
        assert!(entry.is_some());
        assert!(matches!(status, TransactionRejectionStatus::NotRejected), "{status:?}");
    }
}
