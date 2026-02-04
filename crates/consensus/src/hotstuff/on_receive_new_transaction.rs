//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_consensus_types::Decision;
use tari_ootle_common_types::{Epoch, committee::CommitteeInfo};
use tari_ootle_storage::{
    StateStore,
    consensus_models::{TransactionPool, TransactionRecord},
};
use tari_ootle_transaction::TransactionId;
use tokio::sync::mpsc;

use crate::{
    hotstuff::error::HotStuffError,
    messages::MissingTransactionsResponse,
    tracing::TraceTimer,
    traits::{BlockTransactionExecutor, ConsensusSpec},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_receive_new_transaction";

pub struct OnReceiveNewTransaction<TConsensusSpec: ConsensusSpec> {
    store: TConsensusSpec::StateStore,
    transaction_pool: TransactionPool<TConsensusSpec::StateStore>,
    executor: TConsensusSpec::TransactionExecutor,
    tx_missing_transactions: mpsc::UnboundedSender<Vec<TransactionId>>,
}

impl<TConsensusSpec> OnReceiveNewTransaction<TConsensusSpec>
where TConsensusSpec: ConsensusSpec
{
    pub fn new(
        store: TConsensusSpec::StateStore,
        transaction_pool: TransactionPool<TConsensusSpec::StateStore>,
        executor: TConsensusSpec::TransactionExecutor,
        tx_missing_transactions: mpsc::UnboundedSender<Vec<TransactionId>>,
    ) -> Self {
        Self {
            store,
            transaction_pool,
            executor,
            tx_missing_transactions,
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
        info!(target: LOG_TARGET, "Received {} requested transaction(s) for block {} from {:?}", msg.transactions.len(), msg.block_id, from);
        self.store.with_write_tx(|tx| {
            let recs = TransactionRecord::get_any_or_build(&**tx, msg.transactions)?;
            let mut batch = Vec::with_capacity(recs.len());
            debug!(target: LOG_TARGET, "Processing {} requested transactions", recs.len());
            for transaction in recs {
                if let Some(validate_data) =
                    self.validate_new_transaction(tx, current_epoch, transaction, local_committee_info)?
                {
                    debug!(
                        target: LOG_TARGET,
                        "Transaction {} must sequence: {}.",
                        validate_data.transaction.id(),
                        validate_data.must_sequence
                    );
                    batch.push(validate_data);
                }
            }
            debug!(target: LOG_TARGET, "🎱 Adding {} transactions to pool", batch.len());

            self.transaction_pool.insert_new_batched(
                tx,
                local_committee_info.num_preshards(),
                local_committee_info.num_committees(),
                batch
                    .iter()
                    .filter(|data| data.must_sequence)
                    .map(|data| (&data.transaction, data.decision, true)),
            )?;

            // TODO: Could this cause a race-condition? Transaction could be proposed as LocalPrepare before the
            // unparked block is processed (however, if there's a parked block it's probably not our turn to
            // propose). Ideally we remove this channel because it's a work around
            self.tx_missing_transactions
                .send(batch.iter().map(|data| *data.transaction.id()).collect())
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
            let Some(validate_data) =
                self.validate_new_transaction(tx, current_epoch, transaction, local_committee_info)?
            else {
                return Ok(None);
            };
            let NewTransactionValidation {
                transaction,
                decision,
                must_sequence,
            } = validate_data;

            debug!(
                target: LOG_TARGET,
                "Transaction {} must sequence: {}.",
                transaction.id(),
                must_sequence
            );

            if must_sequence {
                self.add_to_pool(tx, &transaction, decision, local_committee_info, true)?;
            }
            Ok(Some(transaction))
        })
    }

    fn validate_new_transaction(
        &self,
        tx: &mut <<TConsensusSpec as ConsensusSpec>::StateStore as StateStore>::WriteTransaction<'_>,
        current_epoch: Epoch,
        rec: TransactionRecord,
        local_committee_info: &CommitteeInfo,
    ) -> Result<Option<NewTransactionValidation>, HotStuffError> {
        if self.transaction_pool.exists(&**tx, rec.id())? {
            return Ok(None);
        }

        // Edge case: a validator sends a transaction that is already finalized as a missing transaction or via
        // propagation
        if rec.is_finalized(&**tx)? {
            warn!(
                target: LOG_TARGET, "Transaction {} is already finalized. Consensus will ignore it.", rec.id()
            );
            return Ok(None);
        }

        let result = self.executor.validate(&**tx, current_epoch, rec.transaction());

        if let Err(err) = result {
            warn!(
                target: LOG_TARGET,
                "Transaction {} received from validator (missing transactions) failed validation and will be ignored: {}", rec.id(), err
            );
            return Ok(None);
        }

        rec.save(tx)?;

        let is_involved_in_inputs_or_all_foreign_inputs = rec.is_involved_in_inputs(local_committee_info) ||
            rec.has_all_foreign_input_pledges(&**tx, local_committee_info)?;

        if !is_involved_in_inputs_or_all_foreign_inputs {
            debug!(
                target: LOG_TARGET,
                "Transaction {} has no local inputs or all foreign inputs. Will sequence once we have received the LocalAccept foreign proposal.",
                rec.id()
            );
        }

        Ok(Some(NewTransactionValidation {
            transaction: rec,
            decision: Decision::Commit,
            // If we dont have the required data, dont add to pool yet. We will add it once we have processed the
            // LocalAccept foreign proposal
            must_sequence: is_involved_in_inputs_or_all_foreign_inputs,
        }))
    }

    fn add_to_pool(
        &self,
        tx: &mut <TConsensusSpec::StateStore as StateStore>::WriteTransaction<'_>,
        transaction: &TransactionRecord,
        decision: Decision,
        local_committee_info: &CommitteeInfo,
        is_ready: bool,
    ) -> Result<(), HotStuffError> {
        info!(
            target: LOG_TARGET,
            "🔥 Adding transaction {} {} ({} input(s)) to pool. Is ready: {}",
            transaction.id(),
            decision,
            transaction.transaction().inputs().len(),
            is_ready
        );
        self.transaction_pool.insert_new(
            tx,
            *transaction.id(),
            decision,
            &transaction.to_initial_evidence(
                local_committee_info.num_preshards(),
                local_committee_info.num_committees(),
            ),
            is_ready,
            transaction.transaction().is_global(),
        )?;
        Ok(())
    }
}

struct NewTransactionValidation {
    pub transaction: TransactionRecord,
    pub decision: Decision,
    pub must_sequence: bool,
}
