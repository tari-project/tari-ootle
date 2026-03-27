//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::trace;
use tari_engine_types::substate::{Substate, SubstateId};
use tari_ootle_common_types::{optional::Optional, shard::Shard};
use tari_ootle_storage::{Ordering, StorageError, consensus_models::SubstateValueFilterFlags};
use tari_state_tree::{Node, NodeKey, StateTreePayload, Version};

use crate::{
    cf_api::CfContext,
    column_families::{state_tree, substate},
    error::RocksDbStorageError,
    reader::ReadOnlyTransaction,
};

const LOG_TARGET: &str = "tari::ootle::state_store_rocksdb::state_tree_iterator";

type BoxedIter<'a> =
    Box<dyn Iterator<Item = Result<((Shard, NodeKey), Node<StateTreePayload>), RocksDbStorageError>> + 'a>;

pub struct LatestSubstateStateTreeIterator<'a> {
    state: IterState,
    tree_query: CfContext<'a, ReadOnlyTransaction<'a>, state_tree::ByShardStateVersionQuery>,
    substate_cf: CfContext<'a, ReadOnlyTransaction<'a>, substate::SubstateCf>,
    shard: Shard,
    state_version: Version,
    iter: Option<BoxedIter<'a>>,
    value_filters: SubstateValueFilterFlags,
}

pub enum IterState {
    NotStarted,
    Taken,
    Iterating,
    Error(StorageError),
    Done,
}

impl IterState {
    pub fn take(&mut self) -> Self {
        std::mem::replace(self, IterState::Taken)
    }

    pub fn put(&mut self, state: Self) {
        if let IterState::Taken = std::mem::replace(self, state) {
            return;
        }
        panic!("IterState::take() was not called before put()");
    }
}

impl<'a> LatestSubstateStateTreeIterator<'a> {
    pub fn new(
        tree_query: CfContext<'a, ReadOnlyTransaction<'a>, state_tree::ByShardStateVersionQuery>,
        substate_cf: CfContext<'a, ReadOnlyTransaction<'a>, substate::SubstateCf>,
        shard: Shard,
        state_version: Version,
        value_filters: SubstateValueFilterFlags,
    ) -> Self {
        Self {
            tree_query,
            substate_cf,
            state_version,
            shard,
            iter: None,
            state: IterState::NotStarted,
            value_filters,
        }
    }
}

impl Iterator for LatestSubstateStateTreeIterator<'_> {
    type Item = Result<(Version, SubstateId, Substate), StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        const OPERATION: &str = "StateTreeIterator::next";
        loop {
            match self.state.take() {
                IterState::NotStarted => {
                    let iter = self.tree_query.query_range_iterator(
                        Ordering::Ascending,
                        (self.shard, self.state_version)..(self.shard, Version::MAX),
                    );
                    self.iter = Some(Box::new(iter));
                    self.state.put(IterState::Iterating);
                },
                IterState::Iterating => {
                    match self.iter.as_mut().expect("Iterating state but no iterator").next() {
                        Some(Ok(((_, node_key), node))) => {
                            match node.leaf() {
                                Some(leaf) => {
                                    let result = self.substate_cf.get(leaf.payload(), OPERATION).optional();
                                    match result {
                                        Ok(Some(substate)) => {
                                            if !self.value_filters.contains_substate(substate.substate_id()) {
                                                // TODO: too bad we cant tell the substate type from the node payload to
                                                // avoid the load
                                                self.state.put(IterState::Iterating);
                                                trace!(
                                                    target: LOG_TARGET,
                                                    "Skipping substate {} due to filter",
                                                    substate.substate_id()
                                                );
                                                continue;
                                            }
                                            if substate.is_destroyed() {
                                                // NODE is likely stale, skip it
                                                // We could try to find the stale node in the DB, but this is costly
                                                // because we store a batch of stale nodes (not keys for each node).
                                                // It is likely not necessary anyway, since we want to exclude substates
                                                // whose values
                                                // that have been deleted in this iterator.
                                                trace!(
                                                    target: LOG_TARGET,
                                                    "Skipping destroyed substate {}",
                                                    substate.substate_id()
                                                );
                                                self.state.put(IterState::Iterating);
                                                continue;
                                            }
                                            let substate_id = substate.substate_id;

                                            match substate.substate_value {
                                                Some(substate_value) => {
                                                    let substate = Substate::new(substate.version, substate_value);
                                                    self.state.put(IterState::Iterating);
                                                    return Some(Ok((node_key.version(), substate_id, substate)));
                                                },
                                                None => {
                                                    self.state.put(IterState::Error(StorageError::DataInconsistency {
                                                        details: format!(
                                                            "Pruned substate value for non-destroyed substate {}v{}",
                                                            substate_id, substate.version
                                                        ),
                                                    }));
                                                    continue;
                                                },
                                            }
                                        },
                                        Ok(None) => {
                                            trace!(
                                                target: LOG_TARGET,
                                                "Substate value for leaf {} (substate: {}) not found. Likely stale node.",
                                                node_key,
                                                leaf.payload()
                                            );

                                            // TODO: currently we never completely prune a downed substate, so this
                                            // should not happen.
                                            // But we'll silently skip instead of erroring because that will likely
                                            // change in the future. NODE is
                                            // likely stale, skip it
                                            self.state.put(IterState::Iterating);
                                            continue;
                                        },
                                        Err(err) => {
                                            self.state.put(IterState::Error(err.into()));
                                            continue;
                                        },
                                    }
                                },
                                None => {
                                    // Skip non-leaf nodes
                                    self.state.put(IterState::Iterating);
                                    continue;
                                },
                            }
                        },
                        Some(Err(err)) => {
                            self.state.put(IterState::Error(err.into()));
                            continue;
                        },
                        None => {
                            self.state.put(IterState::Done);
                            trace!(target: LOG_TARGET, "StateTreeIterator completed");
                            return None;
                        },
                    }
                },
                IterState::Error(err) => {
                    self.state = IterState::Done;
                    return Some(Err(err));
                },
                IterState::Done => return None,
                IterState::Taken => unreachable!("IterState::take() called twice in succession"),
            }
        }
    }
}
