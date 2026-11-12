//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_consensus_types::{BlockId, Decision};
use tari_ootle_common_types::{Epoch, committee::CommitteeInfo};
use tari_ootle_storage::{
    StateStore,
    consensus_models::{TransactionPool, TransactionRecord},
};
use tari_ootle_transaction::{Transaction, TransactionId};
use tokio::sync::mpsc;

use crate::{
    hotstuff::error::HotStuffError,
    tracing::TraceTimer,
    traits::{BlockTransactionValidator, ConsensusSpec},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_receive_new_transaction";

/// Where a transaction entered consensus from, which determines how much validation it still requires.
enum TransactionSource {
    /// Received from our own mempool, where it has already passed full validation.
    OwnMempool,
    /// Loaded from our own transaction store during pool repair, having previously passed full validation.
    Resequenced,
    /// Requested from a peer as a missing transaction; not yet validated locally.
    Requested,
}

pub struct OnReceiveNewTransaction<TConsensusSpec: ConsensusSpec> {
    store: TConsensusSpec::StateStore,
    transaction_pool: TransactionPool<TConsensusSpec::StateStore>,
    validator: TConsensusSpec::TransactionValidator,
    tx_missing_transactions: mpsc::UnboundedSender<Vec<TransactionId>>,
}

impl<TConsensusSpec> OnReceiveNewTransaction<TConsensusSpec>
where
    TConsensusSpec: ConsensusSpec,
{
    pub fn new(
        store: TConsensusSpec::StateStore,
        transaction_pool: TransactionPool<TConsensusSpec::StateStore>,
        validator: TConsensusSpec::TransactionValidator,
        tx_missing_transactions: mpsc::UnboundedSender<Vec<TransactionId>>,
    ) -> Self {
        Self {
            store,
            transaction_pool,
            validator,
            tx_missing_transactions,
        }
    }

    pub async fn process_requested(
        &mut self,
        current_epoch: Epoch,
        from: TConsensusSpec::Addr,
        block_id: BlockId,
        transactions: Vec<Transaction>,
        local_committee_info: &CommitteeInfo,
    ) -> Result<(), HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "OnReceiveRequestedTransactions");
        info!(target: LOG_TARGET, "Received {} requested transaction(s) for block {} from {:?}", transactions.len(), block_id, from);
        process_requested_with(
            &self.store,
            &self.transaction_pool,
            &self.validator,
            &self.tx_missing_transactions,
            current_epoch,
            transactions,
            local_committee_info,
        )
    }

    pub fn try_sequence_transaction(
        &mut self,
        current_epoch: Epoch,
        transaction: TransactionRecord,
        local_committee_info: &CommitteeInfo,
    ) -> Result<Option<TransactionRecord>, HotStuffError> {
        try_sequence_transaction_with(
            &self.store,
            &self.transaction_pool,
            &self.validator,
            current_epoch,
            transaction,
            local_committee_info,
        )
    }

    /// Rebuilds pool records for transactions this node already holds but that are absent from the
    /// transaction pool.
    ///
    /// The pool holds derived per-transaction consensus state and is dropped independently of the
    /// transaction records - a state sync clears it wholesale. A proposal for a transaction whose
    /// record we still hold but whose pool record is gone is unvotable (`TransactionNotInPool`), and
    /// the missing-transaction request path cannot repair it because it only replaces absent records.
    ///
    /// A rebuilt record enters at the `New` pool stage and is inserted ready, so callers must
    /// pass only ids whose command in the block being validated is one the `New` stage serves. An id
    /// carrying a later command gains nothing here - the stage check rejects it either way - and the
    /// ready record would be proposed, putting a stale-stage command in every block this node leads.
    ///
    /// An id this node holds as finalized with an abort is offered too. A proposal that sequences a
    /// previously aborted id must be votable on every node whatever its local records remember, and
    /// a node whose records of the abort are gone sequences it like any new transaction. Safety comes
    /// from `validate_new_transaction_with`, which refuses to sequence an id that has already
    /// committed - by its finalized decision or by its receipt existing in state - and gives an
    /// aborted id a fresh lifecycle, so this can never resurrect a committed transaction.
    ///
    /// Returns the number of transactions put back in the pool.
    pub fn resequence_known_transactions<I: IntoIterator<Item = TransactionId>>(
        &self,
        tx: &mut <<TConsensusSpec as ConsensusSpec>::StateStore as StateStore>::WriteTransaction<'_>,
        current_epoch: Epoch,
        transaction_ids: I,
        local_committee_info: &CommitteeInfo,
    ) -> Result<usize, HotStuffError> {
        resequence_known_transactions_with(
            &self.transaction_pool,
            &self.validator,
            tx,
            current_epoch,
            transaction_ids,
            local_committee_info,
        )
    }
}

fn process_requested_with<TStore, TValidator>(
    store: &TStore,
    transaction_pool: &TransactionPool<TStore>,
    validator: &TValidator,
    tx_missing_transactions: &mpsc::UnboundedSender<Vec<TransactionId>>,
    current_epoch: Epoch,
    transactions: Vec<Transaction>,
    local_committee_info: &CommitteeInfo,
) -> Result<(), HotStuffError>
where
    TStore: StateStore,
    TValidator: BlockTransactionValidator,
{
    store.with_write_tx(|tx| {
        let recs = TransactionRecord::get_any_or_build(&**tx, transactions)?;
        let mut batch = Vec::with_capacity(recs.len());
        debug!(target: LOG_TARGET, "Processing {} requested transactions", recs.len());
        for transaction in recs {
            if let Some(validate_data) = validate_new_transaction_with(
                transaction_pool,
                validator,
                tx,
                current_epoch,
                transaction,
                local_committee_info,
                TransactionSource::Requested,
            )? {
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

        transaction_pool.insert_new_batched(
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
        tx_missing_transactions
            .send(batch.iter().map(|data| *data.transaction.id()).collect())
            .map_err(|_| HotStuffError::InternalChannelClosed {
                context: "process_requested",
            })?;
        Ok(())
    })
}

fn try_sequence_transaction_with<TStore, TValidator>(
    store: &TStore,
    transaction_pool: &TransactionPool<TStore>,
    validator: &TValidator,
    current_epoch: Epoch,
    transaction: TransactionRecord,
    local_committee_info: &CommitteeInfo,
) -> Result<Option<TransactionRecord>, HotStuffError>
where
    TStore: StateStore,
    TValidator: BlockTransactionValidator,
{
    store.with_write_tx(|tx| {
        let Some(validate_data) = validate_new_transaction_with(
            transaction_pool,
            validator,
            tx,
            current_epoch,
            transaction,
            local_committee_info,
            TransactionSource::OwnMempool,
        )?
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
            add_to_pool_with(transaction_pool, tx, &transaction, decision, local_committee_info, true)?;
        }
        Ok(Some(transaction))
    })
}

fn resequence_known_transactions_with<TStore, TValidator, I>(
    transaction_pool: &TransactionPool<TStore>,
    validator: &TValidator,
    tx: &mut TStore::WriteTransaction<'_>,
    current_epoch: Epoch,
    transaction_ids: I,
    local_committee_info: &CommitteeInfo,
) -> Result<usize, HotStuffError>
where
    TStore: StateStore,
    TValidator: BlockTransactionValidator,
    I: IntoIterator<Item = TransactionId>,
{
    let mut unpooled = Vec::new();
    for id in transaction_ids {
        // Cheap membership check first: on the hot path almost every id is already pooled, and
        // this avoids loading its record to find that out.
        if transaction_pool.exists(&**tx, &id)? {
            continue;
        }
        unpooled.push(id);
    }
    if unpooled.is_empty() {
        return Ok(0);
    }

    let (recs, _) = TransactionRecord::get_any(&**tx, unpooled.iter())?;
    let mut batch = Vec::with_capacity(recs.len());
    for rec in recs {
        // A record is only ever persisted by `validate_new_transaction_with` below, after it has passed
        // full validation, so reading one back needs the epoch rules re-checked and nothing more.
        if let Some(validate_data) = validate_new_transaction_with(
            transaction_pool,
            validator,
            tx,
            current_epoch,
            rec,
            local_committee_info,
            TransactionSource::Resequenced,
        )? {
            batch.push(validate_data);
        }
    }

    let num_sequenced = batch.iter().filter(|data| data.must_sequence).count();
    if num_sequenced == 0 {
        return Ok(0);
    }

    info!(
        target: LOG_TARGET,
        "🎱 Re-sequencing {num_sequenced} known transaction(s) that are missing from the pool",
    );
    transaction_pool.insert_new_batched(
        tx,
        local_committee_info.num_preshards(),
        local_committee_info.num_committees(),
        batch
            .iter()
            .filter(|data| data.must_sequence)
            .map(|data| (&data.transaction, data.decision, true)),
    )?;

    Ok(num_sequenced)
}

fn validate_new_transaction_with<TStore, TValidator>(
    transaction_pool: &TransactionPool<TStore>,
    validator: &TValidator,
    tx: &mut TStore::WriteTransaction<'_>,
    current_epoch: Epoch,
    rec: TransactionRecord,
    local_committee_info: &CommitteeInfo,
    source: TransactionSource,
) -> Result<Option<NewTransactionValidation>, HotStuffError>
where
    TStore: StateStore,
    TValidator: BlockTransactionValidator,
{
    if transaction_pool.exists(&**tx, rec.id())? {
        return Ok(None);
    }

    // Consensus refuses to re-sequence an id only once it has committed: a commit consumes the id
    // permanently (its effects, including fee payment, are in state). An abort is a circumstance,
    // not a verdict — it commits nothing, so the same transaction may be sequenced again and
    // either commit under current conditions or abort again. Deciding this on the commit decision
    // rather than on finalization keeps the outcome identical on every node regardless of what
    // its local records remember, so a proposal containing a previously-aborted id can never
    // split votes.
    //
    // Check the finalized marker directly too: a finalized link without a stored execution row yields
    // `get_finalized_decision == None`, and its epoch-index entry must still be cleared if re-sequenced.
    let was_aborted = if TransactionRecord::is_record_finalized(&**tx, rec.id())? {
        if TransactionRecord::get_finalized_decision(&**tx, rec.id())?.is_some_and(|d| d.is_commit()) {
            warn!(
                target: LOG_TARGET, "Transaction {} is already committed. Consensus will ignore it.", rec.id()
            );
            return Ok(None);
        }
        true
    } else {
        false
    };

    // The local records are only a cache: the receipt's existence in synced state proves the id
    // has committed even when this node's records no longer (or never) knew it — freshly synced
    // nodes included. The receipt's shard group is always involved because the receipt is a
    // known output of every transaction; shard groups that do not host it never observe the
    // receipt and rely on the receipt's home group refusing the replay.
    if TransactionRecord::receipt_exists(&**tx, rec.id())? {
        warn!(
            target: LOG_TARGET,
            "Transaction {} has a receipt in state and has therefore already committed. Consensus will ignore it.",
            rec.id()
        );
        return Ok(None);
    }

    // Mempool-originated and locally stored transactions have already passed full validation; their structural
    // properties are immutable, so only the epoch-dependent rules are re-checked. Peer-requested transactions have
    // not been validated locally and are validated in full.
    let result = match source {
        TransactionSource::OwnMempool | TransactionSource::Resequenced => {
            validator.validate_epoch(current_epoch, rec.transaction())
        },
        TransactionSource::Requested => validator.validate_full(current_epoch, rec.transaction()),
    };

    if let Err(err) = result {
        warn!(
            target: LOG_TARGET,
            "Transaction {} failed validation and will be ignored: {}", rec.id(), err
        );
        return Ok(None);
    }

    let is_involved_in_inputs_or_all_foreign_inputs = rec.is_involved_in_inputs(local_committee_info)
        || rec.has_all_foreign_input_pledges(&**tx, local_committee_info)?;

    if !is_involved_in_inputs_or_all_foreign_inputs {
        if matches!(source, TransactionSource::Resequenced) {
            // Pool repair only rebuilds pool entries for local New-stage commands (`must_sequence`). On an output-only
            // shard group, transactions sequence only upon receiving a LocalAccept foreign proposal, so skip them
            // during pool repair without disturbing any finalized abort bookkeeping.
            return Ok(None);
        }
        debug!(
            target: LOG_TARGET,
            "Transaction {} has no local inputs or all foreign inputs. Will sequence once we have received the LocalAccept foreign proposal.",
            rec.id()
        );
    }

    if was_aborted {
        info!(
            target: LOG_TARGET,
            "🔄 Transaction {} was previously finalized as aborted. Removing the previous attempt and \
             sequencing it again.",
            rec.id()
        );
        // The aborted attempt's bookkeeping must not survive into the new lifecycle,
        // otherwise the stale execution could be returned as this id's finalized result
        // after the new attempt finalizes.
        TransactionRecord::remove_finalized(tx, rec.id())?;
    }

    rec.save(tx)?;

    Ok(Some(NewTransactionValidation {
        transaction: rec,
        decision: Decision::Commit,
        // If we dont have the required data, dont add to pool yet. We will add it once we have processed the
        // LocalAccept foreign proposal
        must_sequence: is_involved_in_inputs_or_all_foreign_inputs,
    }))
}

fn add_to_pool_with<TStore: StateStore>(
    transaction_pool: &TransactionPool<TStore>,
    tx: &mut TStore::WriteTransaction<'_>,
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
    transaction_pool.insert_new(
        tx,
        *transaction.id(),
        decision,
        &transaction.to_initial_evidence(
            local_committee_info.num_preshards(),
            local_committee_info.num_committees(),
        ),
        is_ready,
        transaction.transaction().is_global(),
        transaction.transaction().max_epoch(),
        transaction.transaction().calculate_transaction_weight().as_u64(),
    )?;
    Ok(())
}

struct NewTransactionValidation {
    pub transaction: TransactionRecord,
    pub decision: Decision,
    pub must_sequence: bool,
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };

    use tari_common_types::types::PrivateKey;
    use tari_consensus_types::LeafBlock;
    use tari_engine_types::{
        commit_result::{AbortReason, ExecuteResult, FinalizeResult, RejectReason, TransactionResult},
        component::{Component, ComponentBody, ComponentHeader},
        fees::FeeReceipt,
        substate::{SubstateDiff, SubstateId, SubstateValue},
    };
    use tari_ootle_common_types::{
        InputDeclaration, NodeHeight, NumPreshards, ShardGroup, SubstateVersion, VersionedSubstateId, VotePower,
        shard::Shard,
    };
    use tari_ootle_storage::{
        EpochCleanupStep, StateStoreReadTransaction, StateStoreWriteTransaction, StorageError,
        consensus_models::{
            Block, Evidence, SubstateRecord, SubstateTransition, SubstateUpdateBatch, TransactionExecution,
            TransactionPoolRecord,
        },
    };
    use tari_ootle_transaction::Network;
    use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};
    use tari_template_lib_types::{
        ComponentAddress, EntityId, ObjectKey, SubstateOwnerRule, TransactionReceiptAddress,
    };

    use super::*;
    use crate::{
        hotstuff::{block_change_set::ProposedBlockChangeSet, foreign_proposal_processor::get_or_sequence_transaction},
        traits::CertificateStore,
    };

    const NUM_PRESHARDS: NumPreshards = NumPreshards::P256;

    type TestStore = RocksDbStateStore<String>;

    #[derive(Debug, thiserror::Error)]
    #[error("validation rejected transaction")]
    struct ValidationRejected;

    #[derive(Default)]
    struct ToggleValidator {
        reject_epoch: AtomicBool,
        reject_full: AtomicBool,
    }

    impl BlockTransactionValidator for ToggleValidator {
        type Error = ValidationRejected;

        fn validate_full(&self, _current_epoch: Epoch, _transaction: &Transaction) -> Result<(), Self::Error> {
            if self.reject_full.load(Ordering::SeqCst) {
                return Err(ValidationRejected);
            }
            Ok(())
        }

        fn validate_epoch(&self, _current_epoch: Epoch, _transaction: &Transaction) -> Result<(), Self::Error> {
            if self.reject_epoch.load(Ordering::SeqCst) {
                return Err(ValidationRejected);
            }
            Ok(())
        }
    }

    fn single_committee() -> CommitteeInfo {
        CommitteeInfo::new(
            NUM_PRESHARDS,
            1,
            1,
            ShardGroup::all_shards(NUM_PRESHARDS),
            Epoch(1),
            VotePower::of(1),
        )
    }

    fn two_committees() -> (CommitteeInfo, CommitteeInfo) {
        let sg0 = ShardGroup::new(Shard::first(), Shard::from(128));
        let sg1 = ShardGroup::new(Shard::from(129), NUM_PRESHARDS.as_u32().into());
        let c0 = CommitteeInfo::new(NUM_PRESHARDS, 1, 2, sg0, Epoch(1), VotePower::of(1));
        let c1 = CommitteeInfo::new(NUM_PRESHARDS, 1, 2, sg1, Epoch(1), VotePower::of(1));
        (c0, c1)
    }

    fn component_in(shard_group: ShardGroup) -> SubstateId {
        (0x20..=u8::MAX)
            .map(|seed| SubstateId::from(ComponentAddress::from_array([seed; ObjectKey::LENGTH])))
            .find(|id| {
                let shard = VersionedSubstateId::new(id.clone(), SubstateVersion::ZERO).to_shard(NUM_PRESHARDS);
                shard_group.contains(&shard)
            })
            .expect("seed in shard group")
    }

    fn dummy_substate_value() -> SubstateValue {
        Component {
            header: ComponentHeader {
                template_address: Default::default(),
                owner_rule: SubstateOwnerRule::None,
                access_rules: Default::default(),
                entity_id: EntityId::default(),
                upgrade_rule: Default::default(),
            },
            body: ComponentBody {
                state: tari_bor::Value::Null,
            },
        }
        .into()
    }

    fn build_tx_with_input(seed: u64, input: SubstateId) -> TransactionRecord {
        let tx = Transaction::builder_localnet(Epoch(1))
            .add_input(InputDeclaration::write(input))
            .build_and_seal(&PrivateKey::from(seed));
        TransactionRecord::new(tx)
    }

    fn store_finalized_abort(store: &TestStore, rec: &TransactionRecord, epoch: Epoch, include_execution: bool) {
        let pool_rec = TransactionPoolRecord::new_from_transaction(rec.transaction(), Evidence::empty());
        let leaf = LeafBlock {
            block_id: BlockId::new([7u8; 32]),
            height: NodeHeight(2),
            epoch,
            shard_group: ShardGroup::all_shards(NUM_PRESHARDS),
        };
        store
            .with_write_tx(|tx| {
                rec.save(tx)?;
                if include_execution {
                    let reason = AbortReason::ExecutionFailure;
                    TransactionExecution::abort(rec.id(), RejectReason::Abort { reason })
                        .for_block(leaf, *rec.id())
                        .insert_if_required(tx)?;
                }
                TransactionRecord::finalize_all(tx, epoch, [&pool_rec])?;
                Ok::<_, StorageError>(())
            })
            .unwrap();
    }

    fn test_db_options() -> DatabaseOptions {
        DatabaseOptions::default().with_epoch_history_length(1)
    }

    #[test]
    fn rejected_validation_preserves_finalized_abort_bookkeeping() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TestStore::open(tmp.path(), test_db_options()).unwrap();
        let pool = TransactionPool::<TestStore>::new();
        let committee = single_committee();
        let rec = build_tx_with_input(1, component_in(committee.shard_group()));
        let expected_abort = Decision::Abort(AbortReason::ExecutionFailure);

        store_finalized_abort(&store, &rec, Epoch(1), true);

        let validator = ToggleValidator {
            reject_epoch: AtomicBool::new(true),
            reject_full: AtomicBool::new(true),
        };

        // 1. Mempool submission rejected by epoch validation (e.g. max_epoch expired).
        let sequenced =
            try_sequence_transaction_with(&store, &pool, &validator, Epoch(2), rec.clone(), &committee).unwrap();
        assert!(sequenced.is_none());

        // 2. Pool repair on an incoming proposal rejected by epoch validation.
        let resequenced = store
            .with_write_tx(|tx| {
                resequence_known_transactions_with(&pool, &validator, tx, Epoch(2), [*rec.id()], &committee)
            })
            .unwrap();
        assert_eq!(resequenced, 0);

        // 3. Peer-supplied missing transaction rejected by full validation.
        let (tx_missing, mut rx_missing) = mpsc::unbounded_channel();
        process_requested_with(
            &store,
            &pool,
            &validator,
            &tx_missing,
            Epoch(2),
            vec![rec.transaction().clone()],
            &committee,
        )
        .unwrap();
        assert_eq!(rx_missing.try_recv().unwrap(), Vec::<TransactionId>::new());

        store
            .with_read_tx(|tx| {
                assert!(!pool.exists(tx, rec.id()).unwrap());
                assert!(TransactionRecord::is_record_finalized(tx, rec.id())?);
                assert_eq!(
                    TransactionRecord::get_finalized_decision(tx, rec.id())?,
                    Some(expected_abort)
                );
                Ok::<_, StorageError>(())
            })
            .unwrap();

        // Epoch GC must still prune the finalized abort record once Epoch(1) ages past `epoch_history_length`.
        let pruned = store
            .with_write_tx(|tx| tx.epoch_cleanup_step(Epoch(2), EpochCleanupStep::FinalizedTransactions, 100))
            .unwrap();
        assert_eq!(pruned, 1);
        store
            .with_read_tx(|tx| {
                assert!(!TransactionRecord::exists(tx, rec.id())?);
                Ok::<_, StorageError>(())
            })
            .unwrap();
    }

    #[test]
    fn validator_tier_dispatch_and_already_pooled_short_circuit() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TestStore::open(tmp.path(), test_db_options()).unwrap();
        let pool = TransactionPool::<TestStore>::new();
        let committee = single_committee();
        let input_id = component_in(committee.shard_group());
        let rec1 = build_tx_with_input(10, input_id.clone());
        let rec2 = build_tx_with_input(11, input_id);
        let expected_abort = Decision::Abort(AbortReason::ExecutionFailure);

        store_finalized_abort(&store, &rec1, Epoch(1), true);
        store_finalized_abort(&store, &rec2, Epoch(1), true);

        // Epoch validation passes, full validation rejects: `Requested` must reject while `OwnMempool` and
        // `Resequenced` accept without re-running structural checks.
        let validator = ToggleValidator {
            reject_epoch: AtomicBool::new(false),
            reject_full: AtomicBool::new(true),
        };

        let (tx_missing, mut rx_missing) = mpsc::unbounded_channel();
        process_requested_with(
            &store,
            &pool,
            &validator,
            &tx_missing,
            Epoch(2),
            vec![rec1.transaction().clone()],
            &committee,
        )
        .unwrap();
        assert_eq!(rx_missing.try_recv().unwrap(), Vec::<TransactionId>::new());
        store
            .with_read_tx(|tx| {
                assert!(!pool.exists(tx, rec1.id()).unwrap());
                assert!(TransactionRecord::is_record_finalized(tx, rec1.id())?);
                assert_eq!(
                    TransactionRecord::get_finalized_decision(tx, rec1.id())?,
                    Some(expected_abort)
                );
                Ok::<_, StorageError>(())
            })
            .unwrap();

        let sequenced =
            try_sequence_transaction_with(&store, &pool, &validator, Epoch(2), rec1.clone(), &committee).unwrap();
        assert!(sequenced.is_some());

        let resequenced = store
            .with_write_tx(|tx| {
                resequence_known_transactions_with(&pool, &validator, tx, Epoch(2), [*rec2.id()], &committee)
            })
            .unwrap();
        assert_eq!(resequenced, 1);

        let pruned = store
            .with_write_tx(|tx| tx.epoch_cleanup_step(Epoch(2), EpochCleanupStep::FinalizedTransactions, 100))
            .unwrap();
        assert_eq!(pruned, 0);

        store
            .with_read_tx(|tx| {
                for rec in [&rec1, &rec2] {
                    assert!(pool.exists(tx, rec.id()).unwrap());
                    assert!(!TransactionRecord::is_record_finalized(tx, rec.id())?);
                    assert_eq!(TransactionRecord::get_finalized_decision(tx, rec.id())?, None);
                    assert!(TransactionRecord::exists(tx, rec.id())?);
                }
                Ok::<_, StorageError>(())
            })
            .unwrap();

        // Already-pooled transactions short-circuit cleanly across all three entry points.
        let accepting = ToggleValidator::default();
        assert!(
            try_sequence_transaction_with(&store, &pool, &accepting, Epoch(2), rec1.clone(), &committee)
                .unwrap()
                .is_none()
        );
        let resequenced_again = store
            .with_write_tx(|tx| {
                resequence_known_transactions_with(&pool, &accepting, tx, Epoch(2), [*rec1.id()], &committee)
            })
            .unwrap();
        assert_eq!(resequenced_again, 0);
        process_requested_with(
            &store,
            &pool,
            &accepting,
            &tx_missing,
            Epoch(2),
            vec![rec1.transaction().clone()],
            &committee,
        )
        .unwrap();
        assert_eq!(rx_missing.try_recv().unwrap(), Vec::<TransactionId>::new());
    }

    #[test]
    fn receipt_in_state_preserves_local_abort_bookkeeping_and_refuses_replay() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TestStore::open(tmp.path(), DatabaseOptions::default()).unwrap();
        let pool = TransactionPool::<TestStore>::new();
        let validator = ToggleValidator::default();
        let committee = single_committee();
        let rec = build_tx_with_input(2, component_in(committee.shard_group()));

        store_finalized_abort(&store, &rec, Epoch(1), true);

        // Simulate a later commit synced via state sync while local abort bookkeeping is still present.
        let receipt_id = SubstateId::TransactionReceipt(TransactionReceiptAddress::from(*rec.id()));
        let shard = VersionedSubstateId::new(receipt_id.clone(), SubstateVersion::ZERO).to_shard(NUM_PRESHARDS);
        let mut batch = SubstateUpdateBatch::new(Network::LocalNet, Epoch(1));
        batch.with_transition(shard, 0).push(SubstateTransition::Up {
            id: receipt_id,
            version: SubstateVersion::ZERO,
            substate_or_hash: dummy_substate_value().into(),
        });
        store
            .with_write_tx(|tx| SubstateRecord::commit_batch(tx, batch))
            .unwrap();

        let sequenced =
            try_sequence_transaction_with(&store, &pool, &validator, Epoch(2), rec.clone(), &committee).unwrap();
        assert!(sequenced.is_none());

        let resequenced = store
            .with_write_tx(|tx| {
                resequence_known_transactions_with(&pool, &validator, tx, Epoch(2), [*rec.id()], &committee)
            })
            .unwrap();
        assert_eq!(resequenced, 0);

        let (tx_missing, mut rx_missing) = mpsc::unbounded_channel();
        process_requested_with(
            &store,
            &pool,
            &validator,
            &tx_missing,
            Epoch(2),
            vec![rec.transaction().clone()],
            &committee,
        )
        .unwrap();
        assert_eq!(rx_missing.try_recv().unwrap(), Vec::<TransactionId>::new());

        store
            .with_read_tx(|tx| {
                assert!(!pool.exists(tx, rec.id()).unwrap());
                assert!(TransactionRecord::is_record_finalized(tx, rec.id())?);
                assert_eq!(
                    TransactionRecord::get_finalized_decision(tx, rec.id())?,
                    Some(Decision::Abort(AbortReason::ExecutionFailure))
                );
                Ok::<_, StorageError>(())
            })
            .unwrap();
    }

    #[test]
    fn unresubmitted_output_only_abort_preserves_bookkeeping_for_epoch_gc() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TestStore::open(tmp.path(), test_db_options()).unwrap();
        let pool = TransactionPool::<TestStore>::new();
        let validator = ToggleValidator::default();
        let (input_committee, output_only_committee) = two_committees();
        let aborted_out = build_tx_with_input(3, component_in(input_committee.shard_group()));

        store_finalized_abort(&store, &aborted_out, Epoch(1), true);

        // Pool repair on an output-only committee leaves unresubmitted abort bookkeeping intact.
        let resequenced = store
            .with_write_tx(|tx| {
                resequence_known_transactions_with(
                    &pool,
                    &validator,
                    tx,
                    Epoch(2),
                    [*aborted_out.id()],
                    &output_only_committee,
                )
            })
            .unwrap();
        assert_eq!(resequenced, 0);

        store
            .with_read_tx(|tx| {
                assert!(TransactionRecord::is_record_finalized(tx, aborted_out.id())?);
                Ok::<_, StorageError>(())
            })
            .unwrap();

        // Epoch GC cleanly prunes the unresubmitted output-only abort record.
        let pruned = store
            .with_write_tx(|tx| tx.epoch_cleanup_step(Epoch(2), EpochCleanupStep::FinalizedTransactions, 100))
            .unwrap();
        assert_eq!(pruned, 1);
        store
            .with_read_tx(|tx| {
                assert!(!TransactionRecord::exists(tx, aborted_out.id())?);
                Ok::<_, StorageError>(())
            })
            .unwrap();
    }

    #[test]
    fn output_only_committee_clears_finalized_abort_on_resubmission_and_sequences_local_accept() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TestStore::open(tmp.path(), test_db_options()).unwrap();
        let pool = TransactionPool::<TestStore>::new();
        let validator = ToggleValidator::default();
        let (input_committee, output_only_committee) = two_committees();
        let aborted_out = build_tx_with_input(3, component_in(input_committee.shard_group()));

        store_finalized_abort(&store, &aborted_out, Epoch(1), true);

        let zero = Block::zero_block(Network::LocalNet, NUM_PRESHARDS);
        store
            .with_write_tx(|tx| {
                zero.justify().save(tx)?;
                zero.insert(tx)?;
                Ok::<_, StorageError>(())
            })
            .unwrap();
        let leaf = zero.as_leaf();
        let foreign_block_id = BlockId::new([9u8; 32]);

        // Prior to resubmission, a foreign proposal LocalAccept for the stale aborted attempt is dropped.
        let mut change_set = ProposedBlockChangeSet::new(leaf);
        store
            .with_read_tx(|tx| {
                let local_chain = tx.pending_chain_get(&leaf.block_id)?;
                let stale_seq = get_or_sequence_transaction(
                    tx,
                    output_only_committee.num_preshards(),
                    output_only_committee.num_committees(),
                    aborted_out.id(),
                    &local_chain,
                    &foreign_block_id,
                    &mut change_set,
                )?;
                assert!(stale_seq.is_none());
                Ok::<_, HotStuffError>(())
            })
            .unwrap();

        // Resubmission via OwnMempool clears the stale finalized abort marker and saves the transaction record,
        // returning Some(transaction) without adding to pool yet (must_sequence is false).
        let mempool_out = try_sequence_transaction_with(
            &store,
            &pool,
            &validator,
            Epoch(2),
            aborted_out.clone(),
            &output_only_committee,
        )
        .unwrap();
        assert!(mempool_out.is_some());

        store
            .with_read_tx(|tx| {
                assert!(!pool.exists(tx, aborted_out.id()).unwrap());
                assert!(!TransactionRecord::is_record_finalized(tx, aborted_out.id())?);
                assert_eq!(TransactionRecord::get_finalized_decision(tx, aborted_out.id())?, None);
                Ok::<_, StorageError>(())
            })
            .unwrap();

        // Now that the resubmission cleared the stale finalized abort marker, a foreign proposal LocalAccept
        // sequences the transaction into the proposed block change set.
        let mut change_set = ProposedBlockChangeSet::new(leaf);
        store
            .with_read_tx(|tx| {
                let local_chain = tx.pending_chain_get(&leaf.block_id)?;
                let resubmitted_seq = get_or_sequence_transaction(
                    tx,
                    output_only_committee.num_preshards(),
                    output_only_committee.num_committees(),
                    aborted_out.id(),
                    &local_chain,
                    &foreign_block_id,
                    &mut change_set,
                )?;
                assert!(resubmitted_seq.is_some());
                assert_eq!(resubmitted_seq.unwrap().id(), aborted_out.id());
                Ok::<_, HotStuffError>(())
            })
            .unwrap();

        // Resubmission via Requested also clears the abort marker, preserves must_sequence = false,
        // and notifies tx_missing_transactions so parked blocks can unpark.
        store_finalized_abort(&store, &aborted_out, Epoch(1), true);
        let (tx_missing, mut rx_missing) = mpsc::unbounded_channel();
        process_requested_with(
            &store,
            &pool,
            &validator,
            &tx_missing,
            Epoch(2),
            vec![aborted_out.transaction().clone()],
            &output_only_committee,
        )
        .unwrap();
        assert_eq!(rx_missing.try_recv().unwrap(), vec![*aborted_out.id()]);

        store
            .with_read_tx(|tx| {
                assert!(!pool.exists(tx, aborted_out.id()).unwrap());
                assert!(!TransactionRecord::is_record_finalized(tx, aborted_out.id())?);
                Ok::<_, StorageError>(())
            })
            .unwrap();
    }

    #[test]
    fn requested_and_fresh_transactions_sequence_and_notify_missing_channel() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TestStore::open(tmp.path(), test_db_options()).unwrap();
        let pool = TransactionPool::<TestStore>::new();
        let validator = ToggleValidator::default();
        let (input_committee, output_only_committee) = two_committees();
        let input_id = component_in(input_committee.shard_group());
        let (tx_missing, mut rx_missing) = mpsc::unbounded_channel();

        // Fresh (`!was_aborted`) transactions on an output-only committee still persist to `TransactionCf` without
        // pooling and still notify `tx_missing_transactions` when requested from a peer.
        let fresh_req_out = build_tx_with_input(30, input_id.clone());
        process_requested_with(
            &store,
            &pool,
            &validator,
            &tx_missing,
            Epoch(2),
            vec![fresh_req_out.transaction().clone()],
            &output_only_committee,
        )
        .unwrap();
        assert_eq!(rx_missing.try_recv().unwrap(), vec![*fresh_req_out.id()]);

        let fresh_mem_out = build_tx_with_input(31, input_id.clone());
        let accepted = try_sequence_transaction_with(
            &store,
            &pool,
            &validator,
            Epoch(2),
            fresh_mem_out.clone(),
            &output_only_committee,
        )
        .unwrap();
        assert!(accepted.is_some());

        store
            .with_read_tx(|tx| {
                for rec in [&fresh_req_out, &fresh_mem_out] {
                    assert!(TransactionRecord::exists(tx, rec.id())?);
                    assert!(!pool.exists(tx, rec.id()).unwrap());
                    assert!(!TransactionRecord::is_record_finalized(tx, rec.id())?);
                }
                Ok::<_, StorageError>(())
            })
            .unwrap();

        // On an input-involved committee (`must_sequence`), `process_requested_with` pools both fresh and previously
        // aborted transactions and notifies `tx_missing_transactions`.
        let fresh_in = build_tx_with_input(32, input_id.clone());
        let aborted_in = build_tx_with_input(33, input_id);
        store_finalized_abort(&store, &aborted_in, Epoch(1), true);

        process_requested_with(
            &store,
            &pool,
            &validator,
            &tx_missing,
            Epoch(2),
            vec![fresh_in.transaction().clone(), aborted_in.transaction().clone()],
            &input_committee,
        )
        .unwrap();
        let mut notified = rx_missing.try_recv().unwrap();
        notified.sort();
        let mut expected = vec![*fresh_in.id(), *aborted_in.id()];
        expected.sort();
        assert_eq!(notified, expected);

        let pruned_in = store
            .with_write_tx(|tx| tx.epoch_cleanup_step(Epoch(2), EpochCleanupStep::FinalizedTransactions, 100))
            .unwrap();
        assert_eq!(pruned_in, 0);

        store
            .with_read_tx(|tx| {
                for rec in [&fresh_in, &aborted_in] {
                    assert!(pool.exists(tx, rec.id()).unwrap());
                    assert!(!TransactionRecord::is_record_finalized(tx, rec.id())?);
                    assert_eq!(TransactionRecord::get_finalized_decision(tx, rec.id())?, None);
                    assert!(TransactionRecord::exists(tx, rec.id())?);
                }
                Ok::<_, StorageError>(())
            })
            .unwrap();
    }

    #[test]
    fn finalized_link_without_stored_execution_is_cleared_only_when_resequenced() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TestStore::open(tmp.path(), test_db_options()).unwrap();
        let pool = TransactionPool::<TestStore>::new();
        let committee = single_committee();
        let rec = build_tx_with_input(4, component_in(committee.shard_group()));

        store_finalized_abort(&store, &rec, Epoch(1), false);
        store
            .with_read_tx(|tx| {
                assert!(TransactionRecord::is_record_finalized(tx, rec.id())?);
                assert_eq!(TransactionRecord::get_finalized_decision(tx, rec.id())?, None);
                Ok::<_, StorageError>(())
            })
            .unwrap();

        // Rejected validation leaves the finalized link untouched.
        let rejecting = ToggleValidator {
            reject_epoch: AtomicBool::new(true),
            reject_full: AtomicBool::new(false),
        };
        let resequenced = store
            .with_write_tx(|tx| {
                resequence_known_transactions_with(&pool, &rejecting, tx, Epoch(2), [*rec.id()], &committee)
            })
            .unwrap();
        assert_eq!(resequenced, 0);
        store
            .with_read_tx(|tx| {
                assert!(TransactionRecord::is_record_finalized(tx, rec.id())?);
                Ok::<_, StorageError>(())
            })
            .unwrap();

        // Accepted re-sequencing clears both the finalized link and its EpochIndex entry so Epoch(1) GC cannot delete
        // the live pooled transaction's payload.
        let accepting = ToggleValidator::default();
        let resequenced = store
            .with_write_tx(|tx| {
                resequence_known_transactions_with(&pool, &accepting, tx, Epoch(2), [*rec.id()], &committee)
            })
            .unwrap();
        assert_eq!(resequenced, 1);

        let pruned = store
            .with_write_tx(|tx| tx.epoch_cleanup_step(Epoch(2), EpochCleanupStep::FinalizedTransactions, 100))
            .unwrap();
        assert_eq!(pruned, 0);

        store
            .with_read_tx(|tx| {
                assert!(pool.exists(tx, rec.id()).unwrap());
                assert!(!TransactionRecord::is_record_finalized(tx, rec.id())?);
                assert!(TransactionRecord::exists(tx, rec.id())?);
                Ok::<_, StorageError>(())
            })
            .unwrap();
    }

    #[test]
    fn committed_transaction_is_ignored_and_preserves_commit_decision() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TestStore::open(tmp.path(), test_db_options()).unwrap();
        let pool = TransactionPool::<TestStore>::new();
        let validator = ToggleValidator::default();
        let committee = single_committee();
        let rec = build_tx_with_input(5, component_in(committee.shard_group()));
        let pool_rec = TransactionPoolRecord::new_from_transaction(rec.transaction(), Evidence::empty());
        let leaf = LeafBlock {
            block_id: BlockId::new([9u8; 32]),
            height: NodeHeight(2),
            epoch: Epoch(1),
            shard_group: ShardGroup::all_shards(NUM_PRESHARDS),
        };
        let commit_result = ExecuteResult {
            finalize: FinalizeResult::new(
                rec.id().into_array().into(),
                vec![],
                vec![],
                TransactionResult::Accept(SubstateDiff::new()),
                FeeReceipt::builder().build(),
            ),
            execution_time: Duration::ZERO,
            execute_epoch: Some(Epoch(1)),
            wasm_execution_points: 0,
            native_execution_points: 0,
        };

        store
            .with_write_tx(|tx| {
                rec.save(tx)?;
                TransactionExecution::new(commit_result, vec![], vec![])
                    .for_block(leaf, *rec.id())
                    .insert_if_required(tx)?;
                TransactionRecord::finalize_all(tx, Epoch(1), [&pool_rec])?;
                Ok::<_, StorageError>(())
            })
            .unwrap();

        let sequenced =
            try_sequence_transaction_with(&store, &pool, &validator, Epoch(2), rec.clone(), &committee).unwrap();
        assert!(sequenced.is_none());

        let resequenced = store
            .with_write_tx(|tx| {
                resequence_known_transactions_with(&pool, &validator, tx, Epoch(2), [*rec.id()], &committee)
            })
            .unwrap();
        assert_eq!(resequenced, 0);

        let (tx_missing, mut rx_missing) = mpsc::unbounded_channel();
        process_requested_with(
            &store,
            &pool,
            &validator,
            &tx_missing,
            Epoch(2),
            vec![rec.transaction().clone()],
            &committee,
        )
        .unwrap();
        assert_eq!(rx_missing.try_recv().unwrap(), Vec::<TransactionId>::new());

        store
            .with_read_tx(|tx| {
                assert!(!pool.exists(tx, rec.id()).unwrap());
                assert!(TransactionRecord::is_record_finalized(tx, rec.id())?);
                assert_eq!(
                    TransactionRecord::get_finalized_decision(tx, rec.id())?,
                    Some(Decision::Commit)
                );
                Ok::<_, StorageError>(())
            })
            .unwrap();
    }
}
