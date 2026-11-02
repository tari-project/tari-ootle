//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Each test proposes one transaction command that differs from what an honest proposer sends in a single way, and
//! asserts the reason a replica gives for refusing it.

use std::{
    collections::HashMap,
    num::NonZeroU64,
    sync::{Arc, Mutex},
    time::Duration,
};

use tari_common_types::types::PrivateKey;
use tari_consensus_types::{Decision, PcId, ProposalCertificate, ShardGroupAccumulatedData};
use tari_crypto::{ristretto::RistrettoPublicKey, tari_utilities::epoch_time::EpochTime};
use tari_engine_types::{
    commit_result::{AbortReason, ExecuteResult, FeeIntentFallback, FinalizeResult, RejectReason, TransactionResult},
    component::{Component, ComponentBody, ComponentHeader},
    creation_check::CreationCheck,
    fees::{FeeBreakdown, FeeReceipt, FeeSource},
    substate::{Substate, SubstateDiff, SubstateId, SubstateValue},
    transaction_receipt::{FinalizeOutcome, TransactionReceipt},
};
use tari_ootle_common_types::{
    Epoch,
    ExtraData,
    LockIntent,
    NodeHeight,
    NumPreshards,
    ProtocolVersion,
    ShardGroup,
    SubstateLockType,
    SubstateRequirement,
    SubstateVersion,
    VersionedSubstateId,
    VotePower,
    committee::CommitteeInfo,
};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
    consensus_models::{
        Block,
        BlockCommands,
        BlockHeader,
        BlockTransactionExecution,
        Command,
        Evidence,
        InvalidEvidenceReason,
        LeaderFee,
        LocalOnlyAtom,
        LockedEpoch,
        MultiShardAtom,
        NoVoteReason,
        SubstateChange,
        SubstatePledge,
        SubstateRecord,
        SubstateTransition,
        SubstateUpdateBatch,
        TransactionExecution,
        TransactionPoolRecord,
        TransactionPoolStage,
        TransactionPoolStatusUpdate,
        TransactionRecord,
        VersionedSubstateIdLockIntent,
        calculate_leader_fee,
    },
};
use tari_ootle_transaction::{Network, Transaction, TransactionId, TransactionIntent};
use tari_sidechain::{FixedHash, QuorumDecision};
use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};
use tari_template_lib_types::{
    ComponentAddress,
    EntityId,
    ObjectKey,
    SubstateOwnerRule,
    TransactionReceiptAddress,
    constants::TARI_TOKEN,
    crypto::RistrettoPublicKeyBytes,
};
use tempfile::TempDir;

use super::LocalCommandValidator;
use crate::{
    hotstuff::{
        block_change_set::ProposedBlockChangeSet,
        command_derivation::{derive_multi_shard_atom_with_leader_fee, lock_local_accept_execution},
        substate_store::PendingSubstateStore,
        transaction_manager::{
            ConsensusTransactionManager,
            EvidenceOrExecution,
            PreparedTransaction,
            adopt_fee_intent_commit,
        },
    },
    traits::{BlockTransactionExecutor, BlockTransactionExecutorError, CertificateStore, UncommittedChainTemplates},
};

const NETWORK: Network = Network::LocalNet;
const NUM_PRESHARDS: NumPreshards = NumPreshards::P256;
const FEE: u64 = 1_900;

type TestStore = RocksDbStateStore<String>;

/// What the scripted engine produces for a transaction.
#[derive(Debug, Clone, Default)]
struct Script {
    commit: bool,
    /// Substates the transaction creates, besides the next version of each input it writes and its receipt.
    new_outputs: Vec<SubstateId>,
    /// Substates the committed diff downs besides the inputs it writes.
    extra_downs: Vec<VersionedSubstateId>,
    /// Substates the execution reports reading at version zero besides the inputs it is given.
    extra_reads: Vec<SubstateId>,
    /// A committed execution settles a fee intent that creates only the receipt.
    fee_intent: bool,
}

/// Stands in for the engine: a transaction executes as its script says, writing every input it is given that is not
/// read-only.
#[derive(Debug, Clone, Default)]
struct ScriptedExecutor {
    scripts: Arc<Mutex<HashMap<TransactionId, Script>>>,
}

impl ScriptedExecutor {
    fn script(&self, transaction_id: TransactionId, script: Script) {
        self.scripts.lock().unwrap().insert(transaction_id, script);
    }
}

impl BlockTransactionExecutor<TestStore> for ScriptedExecutor {
    fn execute(
        &self,
        transaction: &Transaction,
        _locked_epoch: LockedEpoch,
        _input_fanout: u32,
        resolved_inputs: &HashMap<SubstateRequirement, Substate>,
        _templates: &UncommittedChainTemplates,
        _creation_check: &dyn CreationCheck,
    ) -> Result<TransactionExecution, BlockTransactionExecutorError> {
        let id = transaction.calculate_id();
        let script = self
            .scripts
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .unwrap_or_else(|| panic!("no script for transaction {id}"));

        let inputs = resolved_inputs
            .iter()
            .map(|(requirement, substate)| {
                let lock_type = if requirement.substate_id().is_read_only() {
                    SubstateLockType::Read
                } else {
                    SubstateLockType::Write
                };
                VersionedSubstateIdLockIntent::new(
                    VersionedSubstateId::new(requirement.substate_id().clone(), substate.version()),
                    lock_type,
                    requirement.version().is_some(),
                )
                .with_pledged_value(substate.substate_value())
            })
            .chain(script.extra_reads.iter().map(|id| {
                VersionedSubstateIdLockIntent::new(
                    VersionedSubstateId::new(id.clone(), SubstateVersion::ZERO),
                    SubstateLockType::Read,
                    false,
                )
            }))
            .collect::<Vec<_>>();

        if !script.commit {
            let result = execute_result(
                transaction,
                TransactionResult::Reject(RejectReason::execution_failure_unclassified("scripted abort")),
            );
            return Ok(TransactionExecution::new(result, inputs, Vec::new()));
        }

        let receipt = SubstateId::TransactionReceipt(TransactionReceiptAddress::from(id));
        let mut diff = SubstateDiff::new();
        let mut outputs = Vec::new();
        for input in inputs.iter().filter(|input| input.lock_type().is_write()) {
            let id = input.versioned_substate_id();
            diff.down(id.substate_id().clone(), id.version());
            let next = id.to_next_version();
            diff.up(next.substate_id().clone(), component(next.version()));
            outputs.push(next);
        }
        for id in script.extra_downs {
            diff.down(id.substate_id().clone(), id.version());
        }
        for id in script.new_outputs {
            diff.up(id.clone(), component(SubstateVersion::ZERO));
            outputs.push(VersionedSubstateId::new(id, SubstateVersion::ZERO));
        }
        let receipt_substate = Substate::new(SubstateVersion::ZERO, TransactionReceipt {
            outcome: FinalizeOutcome::Commit,
            diff_summary: Default::default(),
            fee_withdrawals: Default::default(),
            events: Default::default(),
            fee_receipt: fee_receipt(),
            epoch: Epoch::zero(),
            intent_commitment: transaction.calculate_intent_commitment(),
        });
        diff.up(receipt.clone(), receipt_substate.clone());
        outputs.push(VersionedSubstateId::new(receipt.clone(), SubstateVersion::ZERO));
        let fee_intent = script.fee_intent.then(|| {
            let mut fee_intent_diff = SubstateDiff::new();
            fee_intent_diff.up(receipt, receipt_substate);
            FeeIntentFallback {
                diff: fee_intent_diff,
                events: Vec::new(),
                logs: Vec::new(),
                fee_receipt: fee_receipt(),
            }
        });

        let outputs = outputs.into_iter().map(VersionedSubstateIdLockIntent::output).collect();
        let result = execute_result(transaction, TransactionResult::Accept(diff));
        Ok(TransactionExecution::new(result, inputs, outputs).with_fee_intent_fallback(fee_intent))
    }
}

/// A fully-paid fee receipt in which [FEE] is the leader's share, with a 5% burn on top.
fn fee_receipt() -> FeeReceipt {
    let exhaust_burn = FEE / 19;
    let paid = FEE + exhaust_burn;
    let mut cost_breakdown = FeeBreakdown::default();
    cost_breakdown.add(FeeSource::WasmExecution, paid);
    FeeReceipt::builder()
        .with_total_fee_payment(paid)
        .with_total_fees_paid(paid)
        .with_total_fee_overcharge(0)
        .with_cost_breakdown(cost_breakdown)
        .with_exhaust_burn(exhaust_burn)
        .build()
}

fn execute_result(transaction: &Transaction, result: TransactionResult) -> ExecuteResult {
    ExecuteResult {
        finalize: FinalizeResult::new(
            transaction.calculate_id().into_array().into(),
            vec![],
            vec![],
            result,
            fee_receipt(),
        ),
        execution_time: Duration::ZERO,
        execute_epoch: None,
        wasm_execution_points: 1_000,
        native_execution_points: 0,
    }
}

fn component(version: SubstateVersion) -> Substate {
    Substate::new(version, component_value())
}

fn component_value() -> SubstateValue {
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

fn tari_token() -> SubstateId {
    TARI_TOKEN.into()
}

/// A component address. A seed must not repeat the object key of another substate the test uses, such as
/// [TARI_TOKEN]'s, because a substate's address is its object key and version.
fn component_id(seed: u8) -> SubstateId {
    ComponentAddress::from_array([seed; ObjectKey::LENGTH]).into()
}

/// A component address in `shard_group`.
fn component_id_in(shard_group: ShardGroup) -> SubstateId {
    (0x20..=u8::MAX)
        .map(component_id)
        .find(|id| {
            let shard = VersionedSubstateId::new(id.clone(), SubstateVersion::ZERO).to_shard(NUM_PRESHARDS);
            shard_group.contains(&shard)
        })
        .expect("some seed falls in the shard group")
}

/// A chain of the zero block, an uncommitted child of it, and the block a replica evaluates extending that child.
struct Replica {
    store: TestStore,
    executor: ScriptedExecutor,
    manager: ConsensusTransactionManager<ScriptedExecutor, TestStore>,
    committee: CommitteeInfo,
    parent: Block,
    /// The stored block a replica evaluates commands in.
    candidate: Block,
    public_key: RistrettoPublicKey,
    _tmp: TempDir,
}

impl Replica {
    /// A replica whose committee is the only one in the network.
    fn new() -> Self {
        Self::in_committee(ShardGroup::all_shards(NUM_PRESHARDS), 1)
    }

    /// A replica whose committee covers `shard_group` of a network split into `num_committees`.
    fn in_committee(shard_group: ShardGroup, num_committees: u32) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let store = TestStore::open(tmp.path(), DatabaseOptions::default()).unwrap();
        let zero = Block::zero_block(NETWORK, NUM_PRESHARDS);
        let parent = child(&zero, shard_group, BlockCommands::empty());
        let candidate = child(&parent, shard_group, BlockCommands::empty());
        store
            .with_write_tx(|tx| {
                for block in [&zero, &parent, &candidate] {
                    block.justify().save(tx)?;
                    block.insert(tx)?;
                }
                Ok::<_, StorageError>(())
            })
            .unwrap();
        let executor = ScriptedExecutor::default();
        let replica = Self {
            store,
            manager: ConsensusTransactionManager::new(executor.clone(), 2160, usize::MAX),
            executor,
            committee: CommitteeInfo::new(
                NUM_PRESHARDS,
                1,
                num_committees,
                shard_group,
                Epoch::zero(),
                VotePower::of(1),
            ),
            parent,
            candidate,
            public_key: RistrettoPublicKey::default(),
            _tmp: tmp,
        };
        // Every transaction reads the fee resource
        replica.add_substate(&tari_token());
        replica
    }

    /// Commits version zero of `id` to the store.
    fn add_substate(&self, id: &SubstateId) {
        let shard = VersionedSubstateId::new(id.clone(), SubstateVersion::ZERO).to_shard(NUM_PRESHARDS);
        let mut batch = SubstateUpdateBatch::new(NETWORK, Epoch::zero());
        batch.with_transition(shard, 0).push(SubstateTransition::Up {
            id: id.clone(),
            version: SubstateVersion::ZERO,
            substate_or_hash: component_value().into(),
        });
        self.store
            .with_write_tx(|tx| SubstateRecord::commit_batch(tx, batch))
            .unwrap();
    }

    /// Pools a new transaction that writes `inputs` and executes as `script` says.
    /// The transaction's receipt is created in this replica's shard group, so the receipt alone never makes it
    /// cross-shard.
    fn pool_transaction(&self, seed: u64, inputs: &[&SubstateId], script: Script) -> TransactionId {
        self.pool_transaction_with_readiness(seed, inputs, script, true)
    }

    /// Pools a transaction like [`Self::pool_transaction`], ready to leave the New stage only if `is_ready`.
    fn pool_transaction_with_readiness(
        &self,
        seed: u64,
        inputs: &[&SubstateId],
        script: Script,
        is_ready: bool,
    ) -> TransactionId {
        let transaction = (0..)
            .map(|attempt| {
                let mut builder = Transaction::builder_localnet(Epoch(100));
                for input in inputs {
                    builder = builder.add_input(SubstateRequirement::unversioned((*input).clone()));
                }
                builder.build_and_seal(&PrivateKey::from(seed * 1_000 + attempt))
            })
            .find(|transaction| {
                let receipt =
                    SubstateId::TransactionReceipt(TransactionReceiptAddress::from(transaction.calculate_id()));
                self.committee.includes_substate_id(&receipt)
            })
            .expect("some seal key puts the receipt in the shard group");
        let id = transaction.calculate_id();
        self.executor.script(id, script);
        self.store
            .with_write_tx(|tx| {
                TransactionRecord::new(transaction).insert(tx)?;
                tx.transaction_pool_insert_new(id, Decision::Commit, &Evidence::empty(), is_ready, false, Epoch(100), 0)
            })
            .unwrap();
        id
    }

    /// Moves a pooled transaction to `stage` in a pending update at the parent block.
    fn move_to_stage(&self, id: &TransactionId, stage: TransactionPoolStage) {
        let parent = self.parent.as_leaf();
        self.store
            .with_write_tx(|tx| {
                let mut rec = tx
                    .transaction_pool_get_all(usize::MAX)?
                    .into_iter()
                    .find(|rec| rec.id() == id)
                    .unwrap();
                rec.set_next_stage_and_readiness(stage, parent.shard_group()).unwrap();
                tx.transaction_pool_add_pending_update(&parent, &TransactionPoolStatusUpdate::new(rec, false))
            })
            .unwrap();
    }

    /// Evaluates `atoms` in order as the LocalOnly commands of one block, stopping at the first refusal.
    fn evaluate_local_only(&self, atoms: &[LocalOnlyAtom]) -> Evaluation {
        let commands = atoms.iter().cloned().map(Command::LocalOnly).collect::<Vec<_>>();
        self.evaluate(&commands)
    }

    /// Evaluates `commands` in order as the transaction commands of one block, stopping at the first refusal.
    fn evaluate(&self, commands: &[Command]) -> Evaluation {
        self.evaluate_after(commands, |_| {})
    }

    /// Evaluates `commands` like [`Self::evaluate`], in a block whose change set `before` has already updated, as
    /// the block's foreign proposals do.
    fn evaluate_after(&self, commands: &[Command], before: impl FnOnce(&mut ProposedBlockChangeSet)) -> Evaluation {
        let block = &self.candidate;
        let validator = LocalCommandValidator::new(&self.manager, &self.public_key);
        self.store
            .with_read_tx(|tx| {
                let mut substate_store = PendingSubstateStore::init(tx, block.as_leaf(), NUM_PRESHARDS)?;
                let mut change_set = ProposedBlockChangeSet::new(block.as_leaf());
                before(&mut change_set);
                let mut evaluation = Evaluation::default();
                for command in commands {
                    evaluation.no_vote = match command {
                        Command::LocalOnly(atom) => validator.evaluate_local_only_command(
                            tx,
                            block,
                            atom,
                            &self.committee,
                            &mut substate_store,
                            &mut change_set,
                            &mut evaluation.total_leader_fee,
                            &mut evaluation.total_exhaust_burn,
                        ),
                        Command::LocalPrepare(atom) => validator.evaluate_local_prepare_command(
                            tx,
                            block,
                            atom,
                            &self.committee,
                            &mut substate_store,
                            &mut change_set,
                        ),
                        Command::LocalAccept(atom) => validator.evaluate_local_accept_command(
                            tx,
                            block,
                            atom,
                            &self.committee,
                            false,
                            &mut substate_store,
                            &mut change_set,
                        ),
                        Command::AllAccept(atom) => validator.evaluate_all_accept_command(
                            tx,
                            block,
                            atom,
                            &self.committee,
                            &mut substate_store,
                            &mut change_set,
                            &mut evaluation.total_leader_fee,
                            &mut evaluation.total_exhaust_burn,
                        ),
                        command => panic!("unsupported command {command}"),
                    }
                    .unwrap();
                    if evaluation.no_vote.is_some() {
                        break;
                    }
                    let id = command.transaction().expect("transaction command").id();
                    let rec = change_set
                        .get_transaction_pool_record(tx, substate_store.parent_chain(), id)
                        .unwrap();
                    evaluation.stages.push(rec.current_stage());
                    evaluation.executed.push(change_set.transaction_execution(id).is_some());
                }
                let (changes, _) = substate_store.into_parts();
                evaluation.ups = changes
                    .iter()
                    .filter(|change| matches!(change, SubstateChange::Up { .. }))
                    .map(|change| change.substate_id().clone())
                    .collect();
                Ok::<_, StorageError>(evaluation)
            })
            .unwrap()
    }

    /// Evaluates the LocalPrepare `atom` in the parent block and stores what it changes, with the parent justified.
    fn prepare_in_parent(&self, atom: &MultiShardAtom) {
        self.store
            .with_write_tx(|tx| {
                // Reading what a block left pending walks back to the commit block
                tx.blocks_set_qcs(self.parent.parent(), Some(&PcId::zero()), None)
            })
            .unwrap();
        self.store_in_parent(&Command::LocalPrepare(atom.clone()));
    }

    /// Evaluates the LocalAccept `atom` in the parent block and stores what it changes, with the parent justified.
    fn accept_in_parent(&self, atom: &MultiShardAtom) {
        self.store_in_parent(&Command::LocalAccept(atom.clone()));
    }

    fn store_in_parent(&self, command: &Command) {
        let block = &self.parent;
        let validator = LocalCommandValidator::new(&self.manager, &self.public_key);
        self.store
            .with_write_tx(|tx| {
                let mut change_set = ProposedBlockChangeSet::new(block.as_leaf());
                let mut substate_store = PendingSubstateStore::init(&**tx, block.as_leaf(), NUM_PRESHARDS)?;
                let (no_vote, atom) = match command {
                    Command::LocalPrepare(atom) => (
                        validator.evaluate_local_prepare_command(
                            &**tx,
                            block,
                            atom,
                            &self.committee,
                            &mut substate_store,
                            &mut change_set,
                        ),
                        atom,
                    ),
                    Command::LocalAccept(atom) => (
                        validator.evaluate_local_accept_command(
                            &**tx,
                            block,
                            atom,
                            &self.committee,
                            false,
                            &mut substate_store,
                            &mut change_set,
                        ),
                        atom,
                    ),
                    command => panic!("unsupported command {command}"),
                };
                let no_vote = no_vote.unwrap();
                assert!(no_vote.is_none(), "{no_vote:?}");
                let mut rec = change_set
                    .get_transaction_pool_record(&**tx, substate_store.parent_chain(), &atom.id)
                    .unwrap();
                let (_, locks) = substate_store.into_parts();
                change_set.set_substate_locks(locks);
                let evidence = rec.evidence_mut().add_shard_group(self.committee.shard_group());
                if command.is_local_prepare() {
                    evidence.set_prepare_qc(PcId::zero());
                } else {
                    evidence.set_accept_qc(PcId::zero());
                }
                rec.set_ready(rec.is_ready_for_pending_stage(self.committee.shard_group()));
                change_set.set_next_transaction_update(rec).unwrap();
                change_set.save(tx)
            })
            .unwrap();
    }

    /// Makes the candidate the parent and evaluates commands in a new child of it.
    fn extend(&mut self) {
        let candidate = child(&self.candidate, self.committee.shard_group(), BlockCommands::empty());
        self.store
            .with_write_tx(|tx| {
                candidate.justify().save(tx)?;
                candidate.insert(tx)
            })
            .unwrap();
        self.parent = std::mem::replace(&mut self.candidate, candidate);
    }

    /// The pool record of transaction `id` as the candidate block reads it.
    fn pending_record(&self, id: &TransactionId) -> TransactionPoolRecord {
        let block = &self.candidate;
        self.store
            .with_read_tx(|tx| {
                let substate_store = PendingSubstateStore::init(tx, block.as_leaf(), NUM_PRESHARDS)?;
                ProposedBlockChangeSet::new(block.as_leaf()).get_transaction_pool_record(
                    tx,
                    substate_store.parent_chain(),
                    id,
                )
            })
            .unwrap()
    }

    /// The execution of transaction `id` pending in the parent block.
    fn pending_execution(&self, id: &TransactionId) -> TransactionExecution {
        self.store
            .with_read_tx(|tx| BlockTransactionExecution::get_pending_for_block(tx, id, &self.candidate.as_leaf()))
            .unwrap()
            .into_transaction_execution()
    }

    /// Stores pledges of version zero of each of `inputs` from `shard_group` for transaction `id`.
    fn receive_input_pledges(&self, id: &TransactionId, shard_group: ShardGroup, inputs: &[&SubstateId]) {
        let pledges = inputs
            .iter()
            .map(|input| SubstatePledge::Input {
                substate_id: VersionedSubstateId::new((*input).clone(), SubstateVersion::ZERO),
                is_write: !input.is_read_only(),
                substate: Box::new(component_value()),
            })
            .collect();
        self.store
            .with_write_tx(|tx| tx.foreign_substate_pledges_save(id, shard_group, &pledges))
            .unwrap();
    }

    /// The command an honest proposer sends for a New transaction that involves foreign shard groups: LocalAccept if
    /// it aborts or this shard group only creates its outputs, otherwise LocalPrepare.
    fn honest_new_multishard_command(&self, id: &TransactionId) -> Command {
        let block = &self.candidate;
        self.store
            .with_read_tx(|tx| {
                let mut substate_store = PendingSubstateStore::init(tx, block.as_leaf(), NUM_PRESHARDS)?;
                let change_set = ProposedBlockChangeSet::new(block.as_leaf());
                let mut rec = change_set
                    .get_transaction_pool_record(tx, substate_store.parent_chain(), id)
                    .unwrap();
                rec.update_locked_epoch(block.to_locked_epoch());
                let prepared = self
                    .manager
                    .prepare_for_proposal(&mut substate_store, &self.committee, &rec, block.as_leaf(), &change_set)
                    .unwrap();
                let PreparedTransaction::MultiShard(multishard) = prepared else {
                    panic!("transaction {id} did not prepare as multi-shard: {prepared:?}");
                };
                let local = self.committee.shard_group();
                match multishard.into_evidence_or_execution() {
                    EvidenceOrExecution::Execution {
                        execution,
                        fee_intent_only,
                    } => {
                        rec.update_from_execution(NUM_PRESHARDS, self.committee.num_committees(), &execution);
                        if fee_intent_only {
                            rec.set_fee_intent_only(true);
                        }
                        rec.evidence_mut().add_shard_group(local);
                        if execution.decision().is_commit() && !rec.evidence().has_inputs(local) {
                            let involved = NonZeroU64::new(rec.evidence().num_shard_groups() as u64).unwrap();
                            rec.set_leader_fee(rec.calculate_leader_fee(involved));
                        }
                    },
                    EvidenceOrExecution::Evidence { evidence } => {
                        rec.set_local_decision(Decision::Commit).merge_evidence(evidence);
                    },
                }
                let atom = rec.get_current_multi_shard_atom();
                let command = if rec.current_decision().is_abort() || rec.evidence().is_committee_output_only(local) {
                    Command::LocalAccept(atom)
                } else {
                    Command::LocalPrepare(atom)
                };
                Ok::<_, StorageError>(command)
            })
            .unwrap()
    }
}

/// A block of `shard_group` extending `parent` and carrying `commands`.
fn child(parent: &Block, shard_group: ShardGroup, commands: BlockCommands) -> Block {
    let justify = ProposalCertificate::new(
        *parent.id().hash(),
        *parent.id(),
        parent.height(),
        parent.epoch(),
        ShardGroup::all_shards(NUM_PRESHARDS),
        ProposalCertificate::genesis_signatures(),
        QuorumDecision::Accept,
    );
    let header = BlockHeader::create_unsigned(
        NETWORK,
        ProtocolVersion::V0,
        *parent.id(),
        justify.calculate_id(),
        None,
        parent.height() + NodeHeight(1),
        Epoch::zero(),
        shard_group,
        RistrettoPublicKeyBytes::default(),
        FixedHash::zero(),
        &commands,
        0,
        EpochTime::now().as_u64(),
        FixedHash::zero(),
        ShardGroupAccumulatedData::default(),
        ExtraData::new(),
    )
    .unwrap();
    Block::new(header, justify, commands, None)
}

#[derive(Debug, Default)]
struct Evaluation {
    no_vote: Option<NoVoteReason>,
    total_leader_fee: u64,
    total_exhaust_burn: u128,
    /// The pending stage of each transaction voted for.
    stages: Vec<TransactionPoolStage>,
    /// Whether each transaction voted for has an execution in the block.
    executed: Vec<bool>,
    /// The substates the block puts up.
    ups: Vec<SubstateId>,
}

fn commits() -> Script {
    Script {
        commit: true,
        ..Default::default()
    }
}

fn exhaust_burn() -> u64 {
    FEE / 19
}

/// The leader fee of a transaction that commits in one shard group.
fn local_leader_fee() -> LeaderFee {
    calculate_leader_fee(FEE, exhaust_burn(), NonZeroU64::new(1).unwrap())
}

/// What an honest proposer sends for a transaction that commits.
fn committing(id: TransactionId) -> LocalOnlyAtom {
    LocalOnlyAtom {
        id,
        decision: Decision::Commit,
        transaction_fee: FEE,
        leader_fee: Some(local_leader_fee()),
    }
}

/// What an honest proposer sends for a transaction that aborts with `reason`.
fn aborting(id: TransactionId, reason: AbortReason) -> LocalOnlyAtom {
    LocalOnlyAtom {
        id,
        decision: Decision::Abort(reason),
        transaction_fee: 0,
        leader_fee: None,
    }
}

/// Two committees that each cover half the shards, the first holding the fee resource every transaction reads.
fn local_and_foreign_halves() -> (ShardGroup, ShardGroup) {
    let mut groups = NUM_PRESHARDS.all_shard_groups_iter(2);
    let (lower, upper) = (groups.next().unwrap(), groups.next().unwrap());
    let tari_token = VersionedSubstateId::new(tari_token(), SubstateVersion::ZERO).to_shard(NUM_PRESHARDS);
    if lower.contains(&tari_token) {
        (lower, upper)
    } else {
        (upper, lower)
    }
}

mod local_only {
    use super::*;

    #[test]
    fn an_honest_commit_is_voted_for() {
        let replica = Replica::new();
        let input = component_id(0x20);
        replica.add_substate(&input);
        let id = replica.pool_transaction(1, &[&input], commits());

        let evaluation = replica.evaluate_local_only(&[committing(id)]);

        assert!(evaluation.no_vote.is_none(), "{:?}", evaluation.no_vote);
        assert_eq!(evaluation.stages, [TransactionPoolStage::LocalOnly]);
        assert_eq!(evaluation.executed, [true]);
        assert_eq!(evaluation.total_leader_fee, local_leader_fee().fee());
        assert_eq!(
            evaluation.total_exhaust_burn,
            u128::from(local_leader_fee().exhaust_burn())
        );
    }

    #[test]
    fn an_honest_abort_is_voted_for() {
        let replica = Replica::new();
        let input = component_id(0x20);
        replica.add_substate(&input);
        let id = replica.pool_transaction(1, &[&input], Script::default());

        let evaluation = replica.evaluate_local_only(&[aborting(id, AbortReason::ExecutionFailure)]);

        assert!(evaluation.no_vote.is_none(), "{:?}", evaluation.no_vote);
        assert_eq!(evaluation.stages, [TransactionPoolStage::LocalOnly]);
        assert_eq!(evaluation.executed, [true]);
        assert_eq!(evaluation.total_leader_fee, 0);
        assert_eq!(evaluation.total_exhaust_burn, 0);
    }

    #[test]
    fn a_transaction_missing_from_the_pool_is_refused() {
        let replica = Replica::new();

        let evaluation = replica.evaluate_local_only(&[committing(TransactionId::new([9; 32]))]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::TransactionNotInPool)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_transaction_past_the_new_stage_is_refused() {
        let replica = Replica::new();
        let input = component_id(0x20);
        replica.add_substate(&input);
        let id = replica.pool_transaction(1, &[&input], commits());
        replica.move_to_stage(&id, TransactionPoolStage::LocalOnly);

        let evaluation = replica.evaluate_local_only(&[committing(id)]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::StageDisagreement {
                    stage: TransactionPoolStage::LocalOnly,
                    expected: TransactionPoolStage::New,
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn an_abort_proposed_for_a_commit_is_refused() {
        let replica = Replica::new();
        let input = component_id(0x20);
        replica.add_substate(&input);
        let id = replica.pool_transaction(1, &[&input], commits());

        let evaluation = replica.evaluate_local_only(&[aborting(id, AbortReason::ExecutionFailure)]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::DecisionDisagreement {
                    local: Decision::Commit,
                    remote: Decision::Abort(AbortReason::ExecutionFailure),
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_commit_proposed_for_an_execution_abort_is_refused() {
        let replica = Replica::new();
        let input = component_id(0x20);
        replica.add_substate(&input);
        let id = replica.pool_transaction(1, &[&input], Script::default());

        let evaluation = replica.evaluate_local_only(&[committing(id)]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::DecisionDisagreement {
                    remote: Decision::Commit,
                    ..
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_commit_proposed_for_a_missing_input_is_refused() {
        let replica = Replica::new();
        let id = replica.pool_transaction(1, &[&component_id(0x20)], commits());

        let evaluation = replica.evaluate_local_only(&[committing(id)]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::DecisionDisagreement {
                    remote: Decision::Commit,
                    ..
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_different_transaction_fee_is_refused() {
        let replica = Replica::new();
        let input = component_id(0x20);
        replica.add_substate(&input);
        let id = replica.pool_transaction(1, &[&input], commits());

        let evaluation = replica.evaluate_local_only(&[LocalOnlyAtom {
            transaction_fee: FEE + 1,
            ..committing(id)
        }]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::FeeDisagreement)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_commit_without_a_leader_fee_is_refused() {
        let replica = Replica::new();
        let input = component_id(0x20);
        replica.add_substate(&input);
        let id = replica.pool_transaction(1, &[&input], commits());

        let evaluation = replica.evaluate_local_only(&[LocalOnlyAtom {
            leader_fee: None,
            ..committing(id)
        }]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::NoLeaderFee)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_different_leader_fee_is_refused() {
        let replica = Replica::new();
        let input = component_id(0x20);
        replica.add_substate(&input);
        let id = replica.pool_transaction(1, &[&input], commits());

        let evaluation = replica.evaluate_local_only(&[LocalOnlyAtom {
            leader_fee: Some(calculate_leader_fee(FEE, exhaust_burn(), NonZeroU64::new(2).unwrap())),
            ..committing(id)
        }]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::LeaderFeeDisagreement)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_transaction_whose_diff_spends_what_an_earlier_one_in_the_block_spent_is_refused() {
        let replica = Replica::new();
        let spent = component_id(0x20);
        let other = component_id(0x21);
        replica.add_substate(&spent);
        replica.add_substate(&other);
        let first = replica.pool_transaction(1, &[&spent], commits());
        let second = replica.pool_transaction(2, &[&other], Script {
            commit: true,
            extra_downs: vec![VersionedSubstateId::new(spent, SubstateVersion::ZERO)],
            ..Default::default()
        });

        let evaluation = replica.evaluate_local_only(&[committing(first), committing(second)]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::ConflictingTransactionProposed { transaction_id, .. }) if transaction_id == second
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_transaction_with_a_foreign_output_is_refused() {
        let (local, foreign) = local_and_foreign_halves();
        let replica = Replica::in_committee(local, 2);
        let input = component_id_in(local);
        replica.add_substate(&input);
        let id = replica.pool_transaction(1, &[&input], Script {
            commit: true,
            new_outputs: vec![component_id_in(foreign)],
            ..Default::default()
        });

        let evaluation = replica.evaluate_local_only(&[committing(id)]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::LocalOnlyProposedForMultiShard)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_local_only_execution_with_evidence_in_a_foreign_shard_group_is_refused() {
        let (local, foreign) = local_and_foreign_halves();
        let replica = Replica::in_committee(local, 2);
        let input = component_id_in(local);
        let foreign_read = component_id_in(foreign);
        replica.add_substate(&input);
        // Locking a read needs the substate in this replica's store
        replica.add_substate(&foreign_read);
        let id = replica.pool_transaction(1, &[&input], Script {
            commit: true,
            extra_reads: vec![foreign_read],
            ..Default::default()
        });

        let evaluation = replica.evaluate_local_only(&[committing(id)]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::LocalOnlyProposedForMultiShard)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn an_honest_early_abort_is_voted_for() {
        let replica = Replica::new();
        let id = replica.pool_transaction(1, &[&component_id(0x20)], commits());

        let evaluation = replica.evaluate_local_only(&[aborting(id, AbortReason::OneOrMoreInputsNotFound)]);

        assert!(evaluation.no_vote.is_none(), "{:?}", evaluation.no_vote);
        assert_eq!(evaluation.stages, [TransactionPoolStage::LocalOnly]);
    }

    #[test]
    fn an_early_abort_proposed_for_a_different_reason_is_refused() {
        let replica = Replica::new();
        let id = replica.pool_transaction(1, &[&component_id(0x20)], commits());

        let evaluation = replica.evaluate_local_only(&[aborting(id, AbortReason::ExecutionFailure)]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::DecisionDisagreement {
                    local: Decision::Abort(AbortReason::OneOrMoreInputsNotFound),
                    remote: Decision::Abort(AbortReason::ExecutionFailure),
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn an_early_abort_with_a_transaction_fee_is_refused() {
        let replica = Replica::new();
        let id = replica.pool_transaction(1, &[&component_id(0x20)], commits());

        let evaluation = replica.evaluate_local_only(&[LocalOnlyAtom {
            transaction_fee: FEE,
            ..aborting(id, AbortReason::OneOrMoreInputsNotFound)
        }]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::FeeDisagreement)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn an_abort_with_a_leader_fee_is_refused() {
        let replica = Replica::new();
        let input = component_id(0x20);
        replica.add_substate(&input);
        let id = replica.pool_transaction(1, &[&input], Script::default());

        let evaluation = replica.evaluate_local_only(&[LocalOnlyAtom {
            leader_fee: Some(local_leader_fee()),
            ..aborting(id, AbortReason::ExecutionFailure)
        }]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::LeaderFeeDisagreement)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_transaction_waiting_on_foreign_pledges_is_refused() {
        let (replica, id, _) = output_only_waiting_on_pledges();

        let evaluation = replica.evaluate_local_only(&[committing(id)]);

        assert_proposed_before_ready(&evaluation, "LocalOnly");
    }
}

/// The atom of a LocalPrepare or LocalAccept command.
fn multishard_atom(command: &Command) -> MultiShardAtom {
    match command {
        Command::LocalPrepare(atom) | Command::LocalAccept(atom) => atom.clone(),
        command => panic!("{command} is not a multi-shard command"),
    }
}

/// A replica of the committee holding the fee resource, with a pooled transaction that writes a local component and
/// creates one in the other committee. It executes here, so it prepares with an execution.
fn executing_cross_shard() -> (Replica, TransactionId) {
    let (local, foreign) = local_and_foreign_halves();
    let replica = Replica::in_committee(local, 2);
    let input = component_id_in(local);
    replica.add_substate(&input);
    let id = replica.pool_transaction(1, &[&input], Script {
        commit: true,
        new_outputs: vec![component_id_in(foreign)],
        ..Default::default()
    });
    (replica, id)
}

/// A replica of the committee holding the fee resource, with a pooled transaction that also writes a component in the
/// other committee. It cannot execute here, so it prepares by pledging its local inputs.
fn pledging_cross_shard() -> (Replica, TransactionId) {
    let (local, foreign) = local_and_foreign_halves();
    let replica = Replica::in_committee(local, 2);
    let input = component_id_in(local);
    replica.add_substate(&input);
    let id = replica.pool_transaction(1, &[&input, &component_id_in(foreign)], commits());
    (replica, id)
}

/// A replica of the committee holding the fee resource, with a pooled transaction whose local input does not exist
/// and that also writes a component in the other committee. It aborts while preparing.
fn aborting_cross_shard() -> (Replica, TransactionId) {
    let (local, foreign) = local_and_foreign_halves();
    let replica = Replica::in_committee(local, 2);
    let id = replica.pool_transaction(1, &[&component_id_in(local), &component_id_in(foreign)], commits());
    (replica, id)
}

/// A replica of the committee without the fee resource, with a pooled transaction whose inputs are all in the other
/// committee and pledged from it. This committee only creates its receipt.
fn output_only() -> (Replica, TransactionId) {
    let (input_group, local) = local_and_foreign_halves();
    let replica = Replica::in_committee(local, 2);
    let input = component_id_in(input_group);
    let id = replica.pool_transaction(1, &[&input], commits());
    replica.receive_input_pledges(&id, input_group, &[&input, &tari_token()]);
    (replica, id)
}

/// Three committees: the one holding the fee resource, another foreign one, and a local one that holds neither.
fn local_and_two_foreign_thirds() -> (ShardGroup, ShardGroup, ShardGroup) {
    let tari_token = VersionedSubstateId::new(tari_token(), SubstateVersion::ZERO).to_shard(NUM_PRESHARDS);
    let (with_token, others): (Vec<_>, Vec<_>) = NUM_PRESHARDS
        .all_shard_groups_iter(3)
        .partition(|group| group.contains(&tari_token));
    (others[0], with_token[0], others[1])
}

/// A replica of a committee that only creates the receipt of a pooled transaction whose inputs are in two other
/// committees. With `pledged_by_both`, both foreign committees have pledged their inputs and the transaction is ready
/// to leave the New stage. Otherwise only the committee holding the fee resource has pledged, and the transaction is
/// still waiting on the other.
fn output_only_with_two_foreign_input_groups(pledged_by_both: bool) -> (Replica, TransactionId) {
    let (local, first, second) = local_and_two_foreign_thirds();
    let replica = Replica::in_committee(local, 3);
    let first_input = component_id_in(first);
    let second_input = component_id_in(second);
    let id = replica.pool_transaction_with_readiness(1, &[&first_input, &second_input], commits(), pledged_by_both);
    replica.receive_input_pledges(&id, first, &[&first_input, &tari_token()]);
    if pledged_by_both {
        replica.receive_input_pledges(&id, second, &[&second_input]);
    }
    (replica, id)
}

/// A replica waiting on foreign input pledges for an output-only transaction, and the LocalAccept a replica holding
/// every pledge derives for it.
fn output_only_waiting_on_pledges() -> (Replica, TransactionId, MultiShardAtom) {
    let (ready, ready_id) = output_only_with_two_foreign_input_groups(true);
    let command = ready.honest_new_multishard_command(&ready_id);
    assert!(
        matches!(&command, Command::LocalAccept(atom) if atom.decision.is_commit()),
        "{command}"
    );
    let (replica, id) = output_only_with_two_foreign_input_groups(false);
    assert_eq!(id, ready_id);
    (replica, id, multishard_atom(&command))
}

fn assert_proposed_before_ready(evaluation: &Evaluation, expected_command: &str) {
    assert!(
        matches!(
            &evaluation.no_vote,
            Some(NoVoteReason::ProposedBeforeShardGroupsReady {
                stage: TransactionPoolStage::New,
                command,
                ..
            }) if *command == expected_command
        ),
        "{:?}",
        evaluation.no_vote
    );
}

fn assert_voted(evaluation: &Evaluation, stage: TransactionPoolStage, executed: bool) {
    assert!(evaluation.no_vote.is_none(), "{:?}", evaluation.no_vote);
    assert_eq!(evaluation.stages, [stage]);
    assert_eq!(evaluation.executed, [executed]);
}

mod local_prepare {
    use super::*;

    #[test]
    fn an_honest_executed_prepare_is_voted_for() {
        let (replica, id) = executing_cross_shard();
        let command = replica.honest_new_multishard_command(&id);
        assert!(matches!(command, Command::LocalPrepare(_)), "{command}");

        let evaluation = replica.evaluate(&[command]);

        assert_voted(&evaluation, TransactionPoolStage::LocalPrepared, true);
    }

    #[test]
    fn an_honest_pledging_prepare_is_voted_for() {
        let (replica, id) = pledging_cross_shard();
        let command = replica.honest_new_multishard_command(&id);
        assert!(matches!(command, Command::LocalPrepare(_)), "{command}");

        let evaluation = replica.evaluate(&[command]);

        assert_voted(&evaluation, TransactionPoolStage::LocalPrepared, false);
    }

    #[test]
    fn a_transaction_missing_from_the_pool_is_refused() {
        let (replica, id) = executing_cross_shard();
        let atom = MultiShardAtom {
            id: TransactionId::new([9; 32]),
            ..multishard_atom(&replica.honest_new_multishard_command(&id))
        };

        let evaluation = replica.evaluate(&[Command::LocalPrepare(atom)]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::TransactionNotInPool)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_transaction_past_the_new_stage_is_refused() {
        let (replica, id) = executing_cross_shard();
        let command = replica.honest_new_multishard_command(&id);
        replica.move_to_stage(&id, TransactionPoolStage::LocalPrepared);

        let evaluation = replica.evaluate(&[command]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::StageDisagreement {
                    stage: TransactionPoolStage::LocalPrepared,
                    expected: TransactionPoolStage::New,
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_local_only_transaction_is_refused() {
        let replica = Replica::new();
        let input = component_id(0x20);
        replica.add_substate(&input);
        let id = replica.pool_transaction(1, &[&input], commits());

        let evaluation = replica.evaluate(&[Command::LocalPrepare(MultiShardAtom {
            id,
            decision: Decision::Commit,
            evidence: Evidence::empty(),
            transaction_fee: FEE,
            leader_fee: None,
            fee_intent_only: false,
        })]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::MultiShardProposedForLocalOnly)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn an_abort_proposed_for_a_commit_is_refused() {
        let (replica, id) = executing_cross_shard();
        let atom = MultiShardAtom {
            decision: Decision::Abort(AbortReason::ExecutionFailure),
            ..multishard_atom(&replica.honest_new_multishard_command(&id))
        };

        let evaluation = replica.evaluate(&[Command::LocalPrepare(atom)]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::DecisionDisagreement {
                    local: Decision::Commit,
                    remote: Decision::Abort(AbortReason::ExecutionFailure),
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_different_transaction_fee_is_refused() {
        let (replica, id) = executing_cross_shard();
        let honest = multishard_atom(&replica.honest_new_multishard_command(&id));
        let atom = MultiShardAtom {
            transaction_fee: honest.transaction_fee + 1,
            ..honest
        };

        let evaluation = replica.evaluate(&[Command::LocalPrepare(atom)]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::FeeDisagreement)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn different_local_evidence_is_refused() {
        let (replica, id) = pledging_cross_shard();
        let atom = MultiShardAtom {
            evidence: Evidence::empty(),
            ..multishard_atom(&replica.honest_new_multishard_command(&id))
        };

        let evaluation = replica.evaluate(&[Command::LocalPrepare(atom)]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::InvalidEvidence {
                    reason: InvalidEvidenceReason::MismatchedEvidence
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }
}

mod local_accept_from_new {
    use super::*;

    #[test]
    fn an_honest_abort_is_voted_for() {
        let (replica, id) = aborting_cross_shard();
        let command = replica.honest_new_multishard_command(&id);
        assert!(
            matches!(&command, Command::LocalAccept(atom) if atom.decision.is_abort()),
            "{command}"
        );

        let evaluation = replica.evaluate(&[command]);

        assert_voted(&evaluation, TransactionPoolStage::LocalAccepted, true);
    }

    #[test]
    fn an_honest_output_only_commit_is_voted_for() {
        let (replica, id) = output_only();
        let command = replica.honest_new_multishard_command(&id);
        assert!(
            matches!(&command, Command::LocalAccept(atom) if atom.decision.is_commit() && atom.leader_fee.is_some()),
            "{command}"
        );

        let evaluation = replica.evaluate(&[command]);

        assert_voted(&evaluation, TransactionPoolStage::LocalAccepted, true);
    }

    #[test]
    fn an_output_only_commit_pledged_by_every_foreign_group_is_voted_for() {
        let (replica, id) = output_only_with_two_foreign_input_groups(true);
        let command = replica.honest_new_multishard_command(&id);

        let evaluation = replica.evaluate(&[command]);

        assert_voted(&evaluation, TransactionPoolStage::LocalAccepted, true);
    }

    #[test]
    fn an_output_only_commit_waiting_on_foreign_pledges_is_refused() {
        let (replica, _, atom) = output_only_waiting_on_pledges();

        let evaluation = replica.evaluate(&[Command::LocalAccept(atom)]);

        assert_proposed_before_ready(&evaluation, "LocalAccept");
    }

    #[test]
    fn a_transaction_that_prepares_is_refused() {
        let (replica, id) = executing_cross_shard();
        let atom = multishard_atom(&replica.honest_new_multishard_command(&id));

        let evaluation = replica.evaluate(&[Command::LocalAccept(atom)]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::CommandDisagreement {
                    local: "LocalPrepare",
                    remote: "LocalAccept",
                    ..
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_commit_proposed_for_an_abort_is_refused() {
        let (replica, id) = aborting_cross_shard();
        let atom = MultiShardAtom {
            decision: Decision::Commit,
            ..multishard_atom(&replica.honest_new_multishard_command(&id))
        };

        let evaluation = replica.evaluate(&[Command::LocalAccept(atom)]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::DecisionDisagreement {
                    remote: Decision::Commit,
                    ..
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_different_transaction_fee_is_refused() {
        let (replica, id) = output_only();
        let honest = multishard_atom(&replica.honest_new_multishard_command(&id));
        let atom = MultiShardAtom {
            transaction_fee: honest.transaction_fee + 1,
            ..honest
        };

        let evaluation = replica.evaluate(&[Command::LocalAccept(atom)]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::FeeDisagreement)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_commit_without_a_leader_fee_is_refused() {
        let (replica, id) = output_only();
        let atom = MultiShardAtom {
            leader_fee: None,
            ..multishard_atom(&replica.honest_new_multishard_command(&id))
        };

        let evaluation = replica.evaluate(&[Command::LocalAccept(atom)]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::NoLeaderFee)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_different_leader_fee_is_refused() {
        let (replica, id) = output_only();
        let atom = MultiShardAtom {
            leader_fee: Some(local_leader_fee()),
            ..multishard_atom(&replica.honest_new_multishard_command(&id))
        };

        let evaluation = replica.evaluate(&[Command::LocalAccept(atom)]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::LeaderFeeDisagreement)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn an_abort_with_a_leader_fee_is_refused() {
        let (replica, id) = aborting_cross_shard();
        let atom = MultiShardAtom {
            leader_fee: Some(local_leader_fee()),
            ..multishard_atom(&replica.honest_new_multishard_command(&id))
        };

        let evaluation = replica.evaluate(&[Command::LocalAccept(atom)]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::LeaderFeeDisagreement)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn different_evidence_is_refused() {
        let (replica, id) = output_only();
        let atom = MultiShardAtom {
            evidence: Evidence::empty(),
            ..multishard_atom(&replica.honest_new_multishard_command(&id))
        };

        let evaluation = replica.evaluate(&[Command::LocalAccept(atom)]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::InvalidEvidence {
                    reason: InvalidEvidenceReason::MismatchedEvidence
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn an_output_only_fee_intent_commit_retains_rejected_output_shard_groups_in_evidence() {
        let (local, input_group, other_output_group) = local_and_two_foreign_thirds();
        let replica = Replica::in_committee(local, 3);
        let input = component_id_in(input_group);
        let local_output = component_id_in(local);
        let foreign_output = component_id_in(other_output_group);
        // Existing substate at version 0 causes local output lock failure and triggers fee-intent fallback.
        replica.add_substate(&local_output);
        let id = replica.pool_transaction(1, &[&input], Script {
            commit: true,
            new_outputs: vec![local_output, foreign_output],
            fee_intent: true,
            ..Default::default()
        });
        replica.receive_input_pledges(&id, input_group, &[&input, &tari_token()]);

        let command = replica.honest_new_multishard_command(&id);
        let atom = multishard_atom(&command);
        assert!(matches!(command, Command::LocalAccept(_)), "{command}");
        assert_eq!(atom.decision, Decision::Commit);
        assert!(atom.fee_intent_only);
        assert_eq!(atom.evidence.num_shard_groups(), 3);
        assert!(atom.evidence.has(&other_output_group));
        assert_eq!(
            atom.leader_fee,
            Some(calculate_leader_fee(FEE, exhaust_burn(), NonZeroU64::new(3).unwrap()))
        );

        let evaluation = replica.evaluate(&[command]);

        assert_voted(&evaluation, TransactionPoolStage::LocalAccepted, true);
    }

    #[test]
    fn an_output_only_lock_failure_abort_retains_output_shard_groups_in_evidence() {
        let (local, input_group, other_output_group) = local_and_two_foreign_thirds();
        let replica = Replica::in_committee(local, 3);
        let input = component_id_in(input_group);
        let local_output = component_id_in(local);
        let foreign_output = component_id_in(other_output_group);
        replica.add_substate(&local_output);
        let id = replica.pool_transaction(1, &[&input], Script {
            commit: true,
            new_outputs: vec![local_output, foreign_output],
            fee_intent: false,
            ..Default::default()
        });
        replica.receive_input_pledges(&id, input_group, &[&input, &tari_token()]);

        let command = replica.honest_new_multishard_command(&id);
        let atom = multishard_atom(&command);
        assert!(matches!(command, Command::LocalAccept(_)), "{command}");
        assert_eq!(atom.decision, Decision::Abort(AbortReason::LockInputsFailed));
        assert_eq!(atom.evidence.num_shard_groups(), 3);
        assert!(atom.evidence.has(&other_output_group));

        let evaluation = replica.evaluate(&[command]);

        assert_voted(&evaluation, TransactionPoolStage::LocalAccepted, true);
    }
}

/// Rules a replica enforces by comparing a proposed command for a New cross-shard transaction with the one it derives.
mod new_multishard_rules {
    use super::*;

    fn assert_command_disagreement(evaluation: &Evaluation, local: &str, remote: &str) {
        assert!(
            matches!(
                &evaluation.no_vote,
                Some(NoVoteReason::CommandDisagreement { local: l, remote: r, .. }) if *l == local && *r == remote
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_prepare_for_an_abort_is_refused() {
        let (replica, id) = aborting_cross_shard();
        let atom = multishard_atom(&replica.honest_new_multishard_command(&id));

        let evaluation = replica.evaluate(&[Command::LocalPrepare(atom)]);

        assert_command_disagreement(&evaluation, "LocalAccept", "LocalPrepare");
    }

    #[test]
    fn a_prepare_for_an_output_only_transaction_is_refused() {
        let (replica, id) = output_only();
        let atom = multishard_atom(&replica.honest_new_multishard_command(&id));

        let evaluation = replica.evaluate(&[Command::LocalPrepare(atom)]);

        assert_command_disagreement(&evaluation, "LocalAccept", "LocalPrepare");
    }

    #[test]
    fn a_prepare_for_a_transaction_waiting_on_foreign_pledges_is_refused() {
        let (replica, _, atom) = output_only_waiting_on_pledges();

        let evaluation = replica.evaluate(&[Command::LocalPrepare(atom)]);

        assert_proposed_before_ready(&evaluation, "LocalPrepare");
    }

    #[test]
    fn a_prepare_with_a_leader_fee_is_refused() {
        let (replica, id) = executing_cross_shard();
        let atom = MultiShardAtom {
            leader_fee: Some(local_leader_fee()),
            ..multishard_atom(&replica.honest_new_multishard_command(&id))
        };

        let evaluation = replica.evaluate(&[Command::LocalPrepare(atom)]);

        assert!(
            matches!(evaluation.no_vote, Some(NoVoteReason::LeaderFeeDisagreement)),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_prepare_with_different_foreign_evidence_is_refused() {
        let (replica, id) = pledging_cross_shard();
        let mut atom = multishard_atom(&replica.honest_new_multishard_command(&id));
        atom.evidence.add_shard_group(ShardGroup::all_shards(NUM_PRESHARDS));

        let evaluation = replica.evaluate(&[Command::LocalPrepare(atom)]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::InvalidEvidence {
                    reason: InvalidEvidenceReason::MismatchedEvidence
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn an_abort_for_a_different_reason_is_refused() {
        let (replica, id) = aborting_cross_shard();
        let honest = multishard_atom(&replica.honest_new_multishard_command(&id));
        assert_eq!(honest.decision, Decision::Abort(AbortReason::OneOrMoreInputsNotFound));
        let atom = MultiShardAtom {
            decision: Decision::Abort(AbortReason::ExecutionFailure),
            ..honest
        };

        let evaluation = replica.evaluate(&[Command::LocalAccept(atom)]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::DecisionDisagreement {
                    local: Decision::Abort(AbortReason::OneOrMoreInputsNotFound),
                    remote: Decision::Abort(AbortReason::ExecutionFailure),
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn an_accept_with_a_certificate_in_its_evidence_is_refused() {
        let (replica, id) = output_only();
        let mut atom = multishard_atom(&replica.honest_new_multishard_command(&id));
        let (input_group, _) = local_and_foreign_halves();
        atom.evidence.add_shard_group(input_group).set_prepare_qc(PcId::zero());

        let evaluation = replica.evaluate(&[Command::LocalAccept(atom)]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::InvalidEvidence {
                    reason: InvalidEvidenceReason::MismatchedEvidence
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }
}

/// A replica that has prepared and accepted [`executing_cross_shard`]'s transaction, whose execution settles a fee
/// intent, holding the other shard group's pledge of the output it creates there.
fn accepted_cross_shard_with_fee_intent() -> (Replica, TransactionId) {
    let (local, foreign) = local_and_foreign_halves();
    let mut replica = Replica::in_committee(local, 2);
    let input = component_id_in(local);
    replica.add_substate(&input);
    let output = component_id_in(foreign);
    let id = replica.pool_transaction(1, &[&input], Script {
        commit: true,
        new_outputs: vec![output.clone()],
        fee_intent: true,
        ..Default::default()
    });
    let output_pledge = SubstatePledge::Output {
        substate_id: VersionedSubstateId::new(output, SubstateVersion::ZERO),
    };
    replica
        .store
        .with_write_tx(|tx| tx.foreign_substate_pledges_save(&id, foreign, &vec![output_pledge]))
        .unwrap();
    let prepare = multishard_atom(&replica.honest_new_multishard_command(&id));
    replica.prepare_in_parent(&prepare);
    replica.extend();
    let involved = NonZeroU64::new(prepare.evidence.num_shard_groups() as u64).unwrap();
    replica.accept_in_parent(&MultiShardAtom {
        leader_fee: Some(calculate_leader_fee(FEE, exhaust_burn(), involved)),
        ..prepare
    });
    (replica, id)
}

/// The record of transaction `id` after the other shard group's LocalAccept in the block, which commits only the
/// fee intent, and the fee-intent commit it switches the transaction to.
fn adopt_foreign_fee_intent_accept(
    replica: &Replica,
    id: &TransactionId,
) -> (TransactionPoolRecord, TransactionExecution) {
    let (_, foreign) = local_and_foreign_halves();
    let mut rec = replica.pending_record(id);
    let execution = adopt_fee_intent_commit(id, replica.pending_execution(id)).unwrap();
    rec.set_fee_intent_only(true)
        .set_transaction_fee(execution.transaction_fee())
        .set_exhaust_burn(execution.exhaust_burn());
    let involved = NonZeroU64::new(rec.evidence().num_shard_groups() as u64).unwrap();
    let leader_fee = rec.calculate_leader_fee(involved);
    rec.set_leader_fee(leader_fee);
    rec.evidence_mut()
        .add_shard_group(foreign)
        .set_prepare_qc(PcId::zero())
        .set_accept_qc(PcId::zero());
    (rec, execution)
}

fn all_accept_atom(rec: &TransactionPoolRecord) -> MultiShardAtom {
    MultiShardAtom {
        leader_fee: rec.leader_fee().cloned(),
        ..rec.get_current_multi_shard_atom()
    }
}

mod all_accept {
    use super::*;

    /// A foreign LocalAccept earlier in the block that switches the transaction to its fee-intent commit leaves that
    /// commit as the diff the AllAccept applies.
    #[test]
    fn an_accept_after_a_foreign_proposal_in_the_block_switched_it_to_its_fee_intent_applies_the_fee_intent() {
        let (replica, id) = accepted_cross_shard_with_fee_intent();
        let (rec, execution) = adopt_foreign_fee_intent_accept(&replica, &id);
        let receipt = SubstateId::TransactionReceipt(TransactionReceiptAddress::from(id));

        let evaluation = replica.evaluate_after(&[Command::AllAccept(all_accept_atom(&rec))], |change_set| {
            change_set.set_next_transaction_update(rec).unwrap();
            change_set.add_transaction_execution(id, execution).unwrap();
        });

        assert_voted(&evaluation, TransactionPoolStage::AllAccepted, true);
        assert_eq!(evaluation.ups, [receipt]);
    }

    #[test]
    fn an_accept_of_a_fee_intent_only_transaction_holding_its_full_execution_is_refused() {
        let (replica, id) = accepted_cross_shard_with_fee_intent();
        let (rec, _) = adopt_foreign_fee_intent_accept(&replica, &id);

        let evaluation = replica.evaluate_after(&[Command::AllAccept(all_accept_atom(&rec))], |change_set| {
            change_set.set_next_transaction_update(rec).unwrap();
        });

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::FeeIntentExecutionMismatch {
                    fee_intent_only: true,
                    execution_fee_intent_only: false,
                    ..
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }
}

mod local_accept_from_local_prepared {
    use super::*;

    /// The execution a foreign proposal in the block left for the transaction, such as the fee-intent commit another
    /// shard group switched it to, is the one the transaction's LocalAccept locks.
    #[test]
    fn an_accept_after_a_foreign_proposal_in_the_block_replaced_the_execution_is_voted_for() {
        let (replica, id) = executing_cross_shard();
        let prepare = multishard_atom(&replica.honest_new_multishard_command(&id));
        replica.prepare_in_parent(&prepare);
        let execution = replica.pending_execution(&id);
        let involved = NonZeroU64::new(prepare.evidence.num_shard_groups() as u64).unwrap();
        let accept = MultiShardAtom {
            leader_fee: Some(calculate_leader_fee(FEE, exhaust_burn(), involved)),
            ..prepare
        };

        let evaluation = replica.evaluate_after(&[Command::LocalAccept(accept)], |change_set| {
            change_set.add_transaction_execution(id, execution).unwrap();
        });

        assert_voted(&evaluation, TransactionPoolStage::LocalAccepted, true);
    }

    #[test]
    fn an_abort_proposed_for_a_prepared_commit_is_refused() {
        let (replica, id) = executing_cross_shard();
        let prepare = multishard_atom(&replica.honest_new_multishard_command(&id));
        replica.prepare_in_parent(&prepare);
        let involved = NonZeroU64::new(prepare.evidence.num_shard_groups() as u64).unwrap();
        let forged_abort = MultiShardAtom {
            decision: Decision::Abort(AbortReason::ExecutionFailure),
            leader_fee: Some(calculate_leader_fee(FEE, exhaust_burn(), involved)),
            ..prepare
        };

        let evaluation = replica.evaluate(&[Command::LocalAccept(forged_abort)]);

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::DecisionDisagreement {
                    local: Decision::Commit,
                    remote: Decision::Abort(AbortReason::ExecutionFailure),
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    #[test]
    fn a_commit_proposed_for_an_aborted_prepared_transaction_is_refused() {
        let (replica, id) = executing_cross_shard();
        let prepare = multishard_atom(&replica.honest_new_multishard_command(&id));
        replica.prepare_in_parent(&prepare);
        let mut rec = replica.pending_record(&id);
        rec.set_remote_decision(Decision::Abort(AbortReason::ForeignPledgeInputConflict))
            .no_leader_fee();
        let forged_commit = MultiShardAtom {
            decision: Decision::Commit,
            ..rec.get_current_multi_shard_atom()
        };

        let evaluation = replica.evaluate_after(&[Command::LocalAccept(forged_commit)], |change_set| {
            change_set.set_next_transaction_update(rec).unwrap();
        });

        assert!(
            matches!(
                evaluation.no_vote,
                Some(NoVoteReason::DecisionDisagreement {
                    local: Decision::Abort(AbortReason::ForeignPledgeInputConflict),
                    remote: Decision::Commit,
                })
            ),
            "{:?}",
            evaluation.no_vote
        );
    }

    /// A replica in `LocalPrepared` whose transaction executes at `LocalAccept` (after receiving foreign input
    /// pledges) and collides on a local output.
    fn prepared_colliding_on_local_output(fee_intent: bool) -> (Replica, TransactionId, ShardGroup) {
        let (other_output_group, local, foreign_input_group) = local_and_two_foreign_thirds();
        let replica = Replica::in_committee(local, 3);
        let local_input = component_id_in(local);
        let foreign_input = component_id_in(foreign_input_group);
        let local_output = (0x20..=u8::MAX)
            .map(component_id)
            .filter(|id| {
                let shard = VersionedSubstateId::new(id.clone(), SubstateVersion::ZERO).to_shard(NUM_PRESHARDS);
                local.contains(&shard)
            })
            .nth(1)
            .expect("second seed in local shard group");
        let foreign_output = component_id_in(other_output_group);
        replica.add_substate(&local_input);
        let id = replica.pool_transaction(1, &[&local_input, &foreign_input], Script {
            commit: true,
            new_outputs: vec![local_output.clone(), foreign_output],
            fee_intent,
            ..Default::default()
        });
        let prepare = multishard_atom(&replica.honest_new_multishard_command(&id));
        replica.prepare_in_parent(&prepare);
        replica.receive_input_pledges(&id, foreign_input_group, &[&foreign_input]);
        let parent = replica.parent.as_leaf();
        let mut rec = replica.pending_record(&id);
        rec.evidence_mut()
            .add_shard_group(foreign_input_group)
            .set_prepare_qc(PcId::zero());
        rec.set_ready(true);
        replica
            .store
            .with_write_tx(|tx| {
                tx.transaction_pool_add_pending_update(&parent, &TransactionPoolStatusUpdate::new(rec, true))
            })
            .unwrap();
        replica.add_substate(&local_output);
        (replica, id, other_output_group)
    }

    /// Honest `LocalAccept` command produced for a `LocalPrepared` transaction that executes at `LocalAccept`.
    fn honest_accept_from_local_prepared(replica: &Replica, id: &TransactionId) -> MultiShardAtom {
        let block = &replica.candidate;
        let validator = LocalCommandValidator::new(&replica.manager, &replica.public_key);
        replica
            .store
            .with_read_tx(|tx| {
                let mut substate_store = PendingSubstateStore::init(tx, block.as_leaf(), NUM_PRESHARDS)?;
                let change_set = ProposedBlockChangeSet::new(block.as_leaf());
                let mut rec = change_set
                    .get_transaction_pool_record(tx, substate_store.parent_chain(), id)
                    .unwrap();
                let transaction = TransactionRecord::get(tx, id)?;
                let locked_epoch = rec.locked_epoch().unwrap().clone();
                let execution = validator
                    .execute_transaction(
                        &substate_store,
                        &replica.committee,
                        block.as_leaf(),
                        locked_epoch,
                        transaction,
                        &change_set,
                    )
                    .unwrap()
                    .into_transaction_execution();
                lock_local_accept_execution(&mut substate_store, &replica.committee, &mut rec, execution).unwrap();
                Ok::<_, StorageError>(derive_multi_shard_atom_with_leader_fee(&rec).unwrap())
            })
            .unwrap()
    }

    #[test]
    fn an_honest_abort_when_locking_local_outputs_fails_retains_output_shard_groups_and_is_voted_for() {
        let (replica, id, other_output_group) = prepared_colliding_on_local_output(false);
        let honest = honest_accept_from_local_prepared(&replica, &id);
        assert_eq!(honest.decision, Decision::Abort(AbortReason::LockOutputsFailed));
        assert_eq!(honest.evidence.num_shard_groups(), 3);
        assert!(honest.evidence.has(&other_output_group));

        let evaluation = replica.evaluate(&[Command::LocalAccept(honest.clone())]);
        assert_voted(&evaluation, TransactionPoolStage::LocalAccepted, true);

        let wrong_reason = replica.evaluate(&[Command::LocalAccept(MultiShardAtom {
            decision: Decision::Abort(AbortReason::ExecutionFailure),
            ..honest.clone()
        })]);
        assert!(
            matches!(
                wrong_reason.no_vote,
                Some(NoVoteReason::DecisionDisagreement {
                    local: Decision::Abort(AbortReason::LockOutputsFailed),
                    remote: Decision::Abort(AbortReason::ExecutionFailure),
                })
            ),
            "{:?}",
            wrong_reason.no_vote
        );

        let non_zero_fee = replica.evaluate(&[Command::LocalAccept(MultiShardAtom {
            transaction_fee: FEE,
            ..honest.clone()
        })]);
        assert!(
            matches!(non_zero_fee.no_vote, Some(NoVoteReason::FeeDisagreement)),
            "{:?}",
            non_zero_fee.no_vote
        );

        let with_leader_fee = replica.evaluate(&[Command::LocalAccept(MultiShardAtom {
            leader_fee: Some(local_leader_fee()),
            ..honest.clone()
        })]);
        assert!(
            matches!(with_leader_fee.no_vote, Some(NoVoteReason::LeaderFeeDisagreement)),
            "{:?}",
            with_leader_fee.no_vote
        );

        let with_fee_intent_only = replica.evaluate(&[Command::LocalAccept(MultiShardAtom {
            fee_intent_only: true,
            ..honest.clone()
        })]);
        assert!(
            matches!(
                with_fee_intent_only.no_vote,
                Some(NoVoteReason::FeeIntentOnlyDisagreement {
                    local_fee_intent_only: false,
                    proposed_fee_intent_only: true,
                    ..
                })
            ),
            "{:?}",
            with_fee_intent_only.no_vote
        );

        let missing_output_evidence = replica.evaluate(&[Command::LocalAccept(MultiShardAtom {
            evidence: replica.pending_record(&id).evidence().clone(),
            ..honest
        })]);
        assert!(
            matches!(
                missing_output_evidence.no_vote,
                Some(NoVoteReason::InvalidEvidence {
                    reason: InvalidEvidenceReason::MismatchedEvidence
                })
            ),
            "{:?}",
            missing_output_evidence.no_vote
        );
    }

    #[test]
    fn a_fee_intent_commit_when_locking_local_outputs_fails_retains_rejected_output_shard_groups() {
        let (replica, id, other_output_group) = prepared_colliding_on_local_output(true);
        let honest = honest_accept_from_local_prepared(&replica, &id);
        assert_eq!(honest.decision, Decision::Commit);
        assert!(honest.fee_intent_only);
        assert_eq!(honest.evidence.num_shard_groups(), 3);
        assert!(honest.evidence.has(&other_output_group));
        assert_eq!(
            honest.leader_fee,
            Some(calculate_leader_fee(FEE, exhaust_burn(), NonZeroU64::new(3).unwrap()))
        );

        let evaluation = replica.evaluate(&[Command::LocalAccept(honest)]);

        assert_voted(&evaluation, TransactionPoolStage::LocalAccepted, true);
    }
}
