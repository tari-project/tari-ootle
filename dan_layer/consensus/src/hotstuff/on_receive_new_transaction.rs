//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_dan_common_types::{committee::CommitteeInfo, Epoch, TemplateSyncRequest};
use tari_dan_storage::{
    consensus_models::{TransactionPool, TransactionRecord},
    StateStore,
};
use tari_engine_types::commit_result::RejectReason;
use tari_transaction::TransactionId;
use tokio::sync::{broadcast, mpsc};

use crate::{
    hotstuff::{error::HotStuffError, sync_templates},
    messages::MissingTransactionsResponse,
    tracing::TraceTimer,
    traits::{BlockTransactionExecutor, ConsensusSpec},
};

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::on_receive_new_transaction";

pub struct OnReceiveNewTransaction<TConsensusSpec: ConsensusSpec> {
    store: TConsensusSpec::StateStore,
    transaction_pool: TransactionPool<TConsensusSpec::StateStore>,
    executor: TConsensusSpec::TransactionExecutor,
    tx_missing_transactions: mpsc::UnboundedSender<Vec<TransactionId>>,
    tx_template_sync: broadcast::Sender<TemplateSyncRequest>,
}

impl<TConsensusSpec> OnReceiveNewTransaction<TConsensusSpec>
where TConsensusSpec: ConsensusSpec
{
    pub fn new(
        store: TConsensusSpec::StateStore,
        transaction_pool: TransactionPool<TConsensusSpec::StateStore>,
        executor: TConsensusSpec::TransactionExecutor,
        tx_missing_transactions: mpsc::UnboundedSender<Vec<TransactionId>>,
        tx_template_sync: broadcast::Sender<TemplateSyncRequest>,
    ) -> Self {
        Self {
            store,
            transaction_pool,
            executor,
            tx_missing_transactions,
            tx_template_sync,
        }
    }

    pub async fn process_requested(
        &mut self,
        current_epoch: Epoch,
        from: TConsensusSpec::Addr,
        msg: MissingTransactionsResponse,
        local_committee_info: &CommitteeInfo,
    ) -> Result<(), HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "OnReceiveRequestedTransactions");
        info!(target: LOG_TARGET, "Receiving {} requested transactions for block {} from {:?}", msg.transactions.len(), msg.block_id, from);

        // send transactions to check for templates to be synced
        let template_sync_sender = self.tx_template_sync.clone();
        let (tx_template_sync, rx_template_sync) = std::sync::mpsc::channel::<TransactionRecord>();
        tokio::spawn(async move {
            while let Ok(tx) = rx_template_sync.recv() {
                if let Err(error) = sync_templates(template_sync_sender.clone(), &tx).await {
                    error!(target: LOG_TARGET, "Failed to sync templates from transaction: {error:?}");
                }
            }
        });

        self.store.with_write_tx(|tx| {
            let recs = TransactionRecord::get_any_or_build(&**tx, msg.transactions)?;
            let mut batch = Vec::with_capacity(recs.len());
            let tx_template_sync = tx_template_sync.clone();
            for transaction in recs {
                if let Some(transaction_and_is_ready) =
                    self.validate_new_transaction(tx, current_epoch, transaction, local_committee_info)?
                {
                    // trigger any template download if needed before putting into tx pool
                    tx_template_sync.send(transaction_and_is_ready.0.clone())?;
                    batch.push(transaction_and_is_ready);
                }
            }

            self.transaction_pool
                .insert_new_batched(tx, batch.iter().map(|(t, is_ready)| (t, *is_ready)))?;

            // TODO: Could this cause a race-condition? Transaction could be proposed as Prepare before the
            // unparked block is processed (however, if there's a parked block it's probably not our turn to
            // propose). Ideally we remove this channel because it's a work around
            self.tx_missing_transactions
                .send(batch.iter().map(|(t, _)| *t.id()).collect())
                .map_err(|_| HotStuffError::InternalChannelClosed {
                    context: "process_requested",
                })?;
            Ok(())
        })
    }

    pub fn try_sequence_transaction(
        &mut self,
        current_epoch: Epoch,
        transaction: TransactionRecord,
        local_committee_info: &CommitteeInfo,
    ) -> Result<Option<TransactionRecord>, HotStuffError> {
        self.store.with_write_tx(|tx| {
            let Some((transaction, is_ready)) =
                self.validate_new_transaction(tx, current_epoch, transaction, local_committee_info)?
            else {
                return Ok(None);
            };

            self.add_to_pool(tx, &transaction, is_ready)?;
            Ok(Some(transaction))
        })
    }

    fn validate_new_transaction(
        &self,
        tx: &mut <<TConsensusSpec as ConsensusSpec>::StateStore as StateStore>::WriteTransaction<'_>,
        current_epoch: Epoch,
        mut rec: TransactionRecord,
        local_committee_info: &CommitteeInfo,
    ) -> Result<Option<(TransactionRecord, bool)>, HotStuffError> {
        if self.transaction_pool.exists(&**tx, rec.id())? {
            return Ok(None);
        }

        // Edge case: a validator sends a transaction that is already finalized as a missing transaction or via
        // propagation
        if rec.is_finalized() {
            warn!(
                target: LOG_TARGET, "Transaction {} is already finalized. Consensus will ignore it.", rec.id()
            );
            return Ok(None);
        }

        let result = self.executor.validate(&**tx, current_epoch, rec.transaction());

        if let Err(err) = result {
            warn!(
                target: LOG_TARGET,
                "Transaction {} failed validation: {}", rec.id(), err
            );
            rec.set_abort_reason(RejectReason::InvalidTransaction(err.to_string()))
                .save(tx)?;
            return Ok(Some((rec, true)));
        }

        rec.save(tx)?;

        // Check if we're part of the input shard group. If not, only sequence the transaction (is_ready=true, see
        // foreign_proposal_processor) once we have received the LocalAccept foreign proposal.
        let has_some_local_inputs_or_all_foreign_inputs = rec.has_any_local_inputs(local_committee_info) ||
            rec.has_all_foreign_input_pledges(&**tx, local_committee_info)?;

        if !has_some_local_inputs_or_all_foreign_inputs {
            debug!(
                target: LOG_TARGET,
                "Transaction {} has no local inputs or all foreign inputs. Will sequence once we have received the LocalAccept foreign proposal.",
                rec.id()
            );
        }

        Ok(Some((rec, has_some_local_inputs_or_all_foreign_inputs)))
    }

    fn add_to_pool(
        &self,
        tx: &mut <TConsensusSpec::StateStore as StateStore>::WriteTransaction<'_>,
        transaction: &TransactionRecord,
        is_ready: bool,
    ) -> Result<(), HotStuffError> {
        self.transaction_pool
            .insert_new(tx, *transaction.id(), transaction.current_decision(), is_ready)?;
        Ok(())
    }
}
