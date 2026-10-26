//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, fmt, time::Duration};

use tari_consensus_types::BlockId;
use tari_engine_types::substate::Substate;
use tari_ootle_common_types::{SubstateRequirement, SubstateVersion, VersionedSubstateIdRef};
use tari_ootle_storage::{
    StateStoreReadTransaction,
    StorageError,
    consensus_models::{LockedEpoch, TransactionExecution, TransactionRecord},
};
use tari_ootle_transaction::TransactionId;

use crate::hotstuff::substate_store::BlockChanges;

/// What is read and executed for one block ahead of evaluating it in order: the records of its transactions, and
/// executions speculated for them, each usable only by an evaluation that gives the transaction the inputs it was
/// speculated with.
///
/// The engine is a pure function of the transaction, its input substates, the locked epoch, the input fanout and the
/// templates of the chain the block builds on. The fanout and templates are the same for every transaction of the
/// block the memo was speculated for, so a lookup names that block, and compares the input substates and the locked
/// epoch.
///
/// An input the block did not change is named by its version, since a version on the chain the block extends never
/// changes. An input the block changed is named by its value too. Speculation applies changes the block may never
/// make, so two speculated transactions can create the same substate at the same version with different values, and a
/// block rewrites a validator fee pool in place when fees are withdrawn from it.
#[derive(Debug)]
pub struct ExecutionMemo {
    parent_block: BlockId,
    records: HashMap<TransactionId, TransactionRecord>,
    entries: HashMap<TransactionId, SpeculatedExecution>,
    summary: Option<SpeculationSummary>,
    num_hits: usize,
    num_misses: usize,
}

#[derive(Debug)]
struct SpeculatedExecution {
    inputs: SpeculatedInputs,
    locked_epoch: LockedEpoch,
    execution: TransactionExecution,
}

/// The substates an execution read, by the requirement each resolved.
#[derive(Debug)]
pub struct SpeculatedInputs(HashMap<SubstateRequirement, SpeculatedInput>);

#[derive(Debug)]
enum SpeculatedInput {
    /// A version read from the chain the block extends.
    Chain(SubstateVersion),
    /// A version the block changed, and its encoded value.
    Block { version: SubstateVersion, encoded: Vec<u8> },
}

impl SpeculatedInputs {
    /// The inputs `inputs`, as `store` holds them, or `None` if one the block changed cannot be encoded.
    pub fn read<S: BlockChanges>(store: &S, inputs: &HashMap<SubstateRequirement, Substate>) -> Option<Self> {
        inputs
            .iter()
            .map(|(req, substate)| {
                let version = substate.version();
                let input = if store
                    .block_change(VersionedSubstateIdRef::new(req.substate_id(), version))
                    .is_some()
                {
                    SpeculatedInput::Block {
                        version,
                        encoded: tari_bor::encode(substate).ok()?,
                    }
                } else {
                    SpeculatedInput::Chain(version)
                };
                Some((req.clone(), input))
            })
            .collect::<Option<_>>()
            .map(Self)
    }

    /// Whether the inputs at `versions`, as `store` holds them, are exactly these. Two substates with the same encoding
    /// are the same substate. A substate that encodes differently from an equal one only misses.
    fn matches<S: BlockChanges>(&self, store: &S, versions: &[(SubstateRequirement, SubstateVersion)]) -> bool {
        self.0.len() == versions.len() &&
            versions.iter().all(|(req, version)| {
                let Some(input) = self.0.get(req) else {
                    return false;
                };
                match (
                    input,
                    store.block_change(VersionedSubstateIdRef::new(req.substate_id(), *version)),
                ) {
                    (SpeculatedInput::Chain(speculated), None) => speculated == version,
                    (
                        SpeculatedInput::Block {
                            version: speculated,
                            encoded,
                        },
                        Some(change),
                    ) => {
                        speculated == version &&
                            change.up_substate().is_some_and(|substate| {
                                tari_bor::encode(substate).is_ok_and(|bytes| bytes == *encoded)
                            })
                    },
                    _ => false,
                }
            })
    }
}

/// How a block's transactions were speculated.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpeculationSummary {
    pub num_candidates: usize,
    pub num_speculated: usize,
    pub num_units: usize,
    /// The number of transactions in the unit with the most, and their summed weight.
    pub longest_unit: (usize, u64),
    pub num_threads: usize,
    pub elapsed: Duration,
    /// Whether speculation stopped at its time or execution budget before every unit was done.
    pub is_budget_spent: bool,
}

impl ExecutionMemo {
    /// A memo for the block that builds on `parent_block`, holding the records of the block's transactions read so far.
    pub fn new(parent_block: BlockId, records: HashMap<TransactionId, TransactionRecord>) -> Self {
        Self {
            parent_block,
            records,
            entries: HashMap::new(),
            summary: None,
            num_hits: 0,
            num_misses: 0,
        }
    }

    /// The record of `transaction_id`, if it was read for the block.
    pub fn record(&self, transaction_id: &TransactionId) -> Option<&TransactionRecord> {
        self.records.get(transaction_id)
    }

    /// The record of `transaction_id`: the one read for the block, or else the one `tx` holds. A record is taken from
    /// the memo at most once.
    pub fn take_record<TTx: StateStoreReadTransaction>(
        &mut self,
        tx: &TTx,
        transaction_id: &TransactionId,
    ) -> Result<TransactionRecord, StorageError> {
        match self.records.remove(transaction_id) {
            Some(record) => Ok(record),
            None => TransactionRecord::get(tx, transaction_id),
        }
    }

    /// Records the execution of `transaction_id` against `inputs` at `locked_epoch`.
    pub fn insert(
        &mut self,
        transaction_id: TransactionId,
        inputs: SpeculatedInputs,
        locked_epoch: LockedEpoch,
        execution: TransactionExecution,
    ) {
        self.entries.insert(transaction_id, SpeculatedExecution {
            inputs,
            locked_epoch,
            execution,
        });
    }

    pub fn set_summary(&mut self, summary: SpeculationSummary) {
        self.summary = Some(summary);
    }

    /// Whether the block's transactions were speculated.
    pub fn is_speculated(&self) -> bool {
        self.summary.is_some()
    }

    /// The execution speculated for `transaction_id` in the block that builds on `parent_block`, if it was speculated
    /// at `locked_epoch` with exactly the inputs at `versions` that `store` holds. An execution is taken at most once.
    pub fn take<S: BlockChanges>(
        &mut self,
        store: &S,
        parent_block: &BlockId,
        transaction_id: &TransactionId,
        versions: &[(SubstateRequirement, SubstateVersion)],
        locked_epoch: &LockedEpoch,
    ) -> Option<TransactionExecution> {
        if *parent_block != self.parent_block {
            return None;
        }
        let entry = self.entries.remove(transaction_id)?;
        if entry.locked_epoch == *locked_epoch && entry.inputs.matches(store, versions) {
            self.num_hits += 1;
            Some(entry.execution)
        } else {
            self.num_misses += 1;
            None
        }
    }

    /// The executions not yet taken.
    pub fn num_untaken(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn num_hits(&self) -> usize {
        self.num_hits
    }

    #[cfg(test)]
    pub fn num_misses(&self) -> usize {
        self.num_misses
    }
}

impl fmt::Display for ExecutionMemo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(summary) = &self.summary {
            write!(
                f,
                "speculated {} of {} transaction(s) in {} unit(s) on {} thread(s) in {:.2?}{}, longest unit {} \
                 transaction(s) of weight {}; ",
                summary.num_speculated,
                summary.num_candidates,
                summary.num_units,
                summary.num_threads,
                summary.elapsed,
                if summary.is_budget_spent { " (budget spent)" } else { "" },
                summary.longest_unit.0,
                summary.longest_unit.1,
            )?;
        }
        write!(
            f,
            "{} hit(s), {} miss(es), {} unused",
            self.num_hits,
            self.num_misses,
            self.entries.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use tari_engine_types::{commit_result::RejectReason, non_fungible::NonFungibleContainer, substate::SubstateId};
    use tari_ootle_common_types::{Epoch, SubstateAddress, ToSubstateAddress, shard::Shard};
    use tari_ootle_storage::consensus_models::SubstateChange;
    use tari_template_lib_types::{ComponentAddress, ObjectKey};

    use super::*;

    /// The changes of a block, with no chain beneath them.
    #[derive(Default)]
    struct Changes(HashMap<SubstateAddress, SubstateChange>);

    impl Changes {
        fn up(mut self, substate: Substate) -> Self {
            let address =
                VersionedSubstateIdRef::new(requirement().substate_id(), substate.version()).to_substate_address();
            self.0.insert(address, SubstateChange::Up {
                id: requirement().into_substate_id(),
                shard: Shard::first(),
                substate: Box::new(substate),
            });
            self
        }
    }

    impl BlockChanges for Changes {
        fn block_change(&self, id: VersionedSubstateIdRef<'_>) -> Option<&SubstateChange> {
            self.0.get(&id.to_substate_address())
        }
    }

    fn requirement() -> SubstateRequirement {
        SubstateRequirement::unversioned(SubstateId::from(ComponentAddress::from_array([1; ObjectKey::LENGTH])))
    }

    fn substate_at(version: u64) -> Substate {
        Substate::new(SubstateVersion::new(version), NonFungibleContainer::no_data())
    }

    fn burnt_at(version: u64) -> Substate {
        let mut burnt = NonFungibleContainer::no_data();
        burnt.burn();
        Substate::new(SubstateVersion::new(version), burnt)
    }

    fn at(version: u64) -> Vec<(SubstateRequirement, SubstateVersion)> {
        vec![(requirement(), SubstateVersion::new(version))]
    }

    fn pinned_to(epoch: u64) -> LockedEpoch {
        LockedEpoch::new(Epoch(epoch), Default::default(), Default::default())
    }

    /// A memo of an execution that read `substate` as `speculated` held it, at `locked_epoch`.
    fn memo_with(speculated: &Changes, substate: Substate, locked_epoch: LockedEpoch) -> ExecutionMemo {
        let mut memo = ExecutionMemo::new(BlockId::zero(), HashMap::new());
        let execution = TransactionExecution::abort(
            &TransactionId::default(),
            RejectReason::execution_failure_unclassified("test"),
        );
        let inputs = HashMap::from([(requirement(), substate)]);
        memo.insert(
            TransactionId::default(),
            SpeculatedInputs::read(speculated, &inputs).unwrap(),
            locked_epoch,
            execution,
        );
        memo
    }

    fn take(
        memo: &mut ExecutionMemo,
        store: &Changes,
        versions: &[(SubstateRequirement, SubstateVersion)],
        locked_epoch: LockedEpoch,
    ) -> Option<TransactionExecution> {
        memo.take(
            store,
            &BlockId::zero(),
            &TransactionId::default(),
            versions,
            &locked_epoch,
        )
    }

    #[test]
    fn the_same_chain_version_and_locked_epoch_hit() {
        let mut memo = memo_with(&Changes::default(), substate_at(3), pinned_to(5));
        assert!(take(&mut memo, &Changes::default(), &at(3), pinned_to(5)).is_some());
        assert_eq!(memo.num_hits(), 1);
        assert_eq!(memo.num_untaken(), 0);
    }

    #[test]
    fn a_different_chain_version_misses() {
        let mut memo = memo_with(&Changes::default(), substate_at(3), pinned_to(5));
        assert!(take(&mut memo, &Changes::default(), &at(4), pinned_to(5)).is_none());
        assert_eq!(memo.num_misses(), 1);
    }

    #[test]
    fn the_same_value_the_block_changed_hits() {
        let changes = Changes::default().up(substate_at(3));
        let mut memo = memo_with(&changes, substate_at(3), pinned_to(5));
        assert!(take(&mut memo, &changes, &at(3), pinned_to(5)).is_some());
    }

    #[test]
    fn a_different_value_the_block_changed_at_the_same_version_misses() {
        let speculated = Changes::default().up(substate_at(3));
        let mut memo = memo_with(&speculated, substate_at(3), pinned_to(5));
        let evaluated = Changes::default().up(burnt_at(3));
        assert!(take(&mut memo, &evaluated, &at(3), pinned_to(5)).is_none());
        assert_eq!(memo.num_misses(), 1);
    }

    #[test]
    fn a_chain_version_the_block_changed_misses() {
        let mut memo = memo_with(&Changes::default(), substate_at(3), pinned_to(5));
        let evaluated = Changes::default().up(substate_at(3));
        assert!(take(&mut memo, &evaluated, &at(3), pinned_to(5)).is_none());
    }

    #[test]
    fn a_version_speculation_changed_that_the_block_did_not_misses() {
        let speculated = Changes::default().up(substate_at(0));
        let mut memo = memo_with(&speculated, substate_at(0), pinned_to(5));
        assert!(take(&mut memo, &Changes::default(), &at(0), pinned_to(5)).is_none());
    }

    #[test]
    fn a_different_input_set_misses() {
        let mut memo = memo_with(&Changes::default(), substate_at(3), pinned_to(5));
        let mut more = at(3);
        more.push((
            SubstateRequirement::unversioned(SubstateId::from(ComponentAddress::from_array([2; ObjectKey::LENGTH]))),
            SubstateVersion::ZERO,
        ));
        assert!(take(&mut memo, &Changes::default(), &more, pinned_to(5)).is_none());
    }

    #[test]
    fn a_different_locked_epoch_misses() {
        let mut memo = memo_with(&Changes::default(), substate_at(3), pinned_to(5));
        assert!(take(&mut memo, &Changes::default(), &at(3), pinned_to(4)).is_none());
        assert_eq!(memo.num_misses(), 1);
    }

    #[test]
    fn a_lookup_for_another_block_misses() {
        let mut memo = memo_with(&Changes::default(), substate_at(3), pinned_to(5));
        let other = BlockId::from([1u8; 32]);
        let hit = memo.take(
            &Changes::default(),
            &other,
            &TransactionId::default(),
            &at(3),
            &pinned_to(5),
        );
        assert!(hit.is_none());
    }
}
