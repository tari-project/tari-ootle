//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use log::*;
use tari_engine_types::substate::{Substate, SubstateDiff, SubstateId, SubstateValue};
use tari_ootle_common_types::{
    NumPreshards,
    SubstateAddress,
    SubstateRequirement,
    SubstateVersion,
    ToSubstateAddress,
    VersionedSubstateId,
    VersionedSubstateIdRef,
    displayable::Displayable,
    optional::Optional,
    substate_type::SubstateType,
};
use tari_ootle_storage::consensus_models::SubstateChange;

use super::{LatestSubstateVersion, LockFailedError, SubstateStoreError};

const LOG_TARGET: &str = "tari::ootle::hotstuff::substate_store::change_log";

/// The state beneath a [`ChangeLog`]: what every lookup falls through to when the log holds no change for the
/// substate.
pub(super) trait SubstateLayer {
    fn num_preshards(&self) -> NumPreshards;

    /// The substate at exactly `id`. Fails with `SubstateNotFound` unless that version is UP.
    fn get_substate(&self, id: VersionedSubstateIdRef<'_>) -> Result<Substate, SubstateStoreError>;

    /// Whether the version `id` names is UP, or `None` if it has never existed.
    fn is_version_up(&self, id: VersionedSubstateIdRef<'_>) -> Result<Option<bool>, SubstateStoreError>;

    /// The most recent change to `id`. Fails with `SubstateNotFound` if no version of it has ever existed.
    fn latest_change(&self, id: &SubstateId) -> Result<SubstateChange, SubstateStoreError>;

    /// The most recent version of `id`. Fails with `SubstateNotFound` if no version of it has ever existed.
    fn latest_version(&self, id: &SubstateId) -> Result<LatestSubstateVersion, SubstateStoreError>;

    /// The most recent change to `id` made by the block being built. An in-place update rewrites it at its version.
    fn block_head_change(&self, id: &SubstateId) -> Option<&SubstateChange>;
}

/// Substate changes in the order they were made, over a [`SubstateLayer`]. Every lookup sees the most recent change
/// first, so each change reads the ones before it.
#[derive(Debug, Clone, Default)]
pub(super) struct ChangeLog {
    /// Map from substate address to the index in `changes` of the corresponding change
    pending: HashMap<SubstateAddress, usize>,
    /// Map from substate id to the index in `changes` of the latest change
    head: HashMap<SubstateId, usize>,
    /// Append only list of changes ordered oldest to newest
    changes: Vec<SubstateChange>,
    /// What the `put_diff` in progress has overwritten
    undo: Option<UndoLog>,
}

/// The entries a [`ChangeLog::put_diff`] overwrites, in the order it overwrote them, so that a failure part way can
/// restore the log as it was before the diff.
#[derive(Debug, Clone, Default)]
struct UndoLog {
    /// The number of changes before the diff
    num_changes: usize,
    /// Each `pending` entry the diff set, with its previous index
    pending: Vec<(SubstateAddress, Option<usize>)>,
    /// Each `head` entry the diff set, with its previous index
    head: Vec<(SubstateId, Option<usize>)>,
    /// Each change from before the diff that the diff rewrote in place, with its previous value
    rewritten: Vec<(usize, SubstateChange)>,
}

impl ChangeLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn changes(&self) -> &Vec<SubstateChange> {
        &self.changes
    }

    pub fn into_changes(self) -> Vec<SubstateChange> {
        self.changes
    }

    pub fn get_pending(&self, addr: &SubstateAddress) -> Option<&SubstateChange> {
        self.pending
            .get(addr)
            .map(|&pos| self.changes.get(pos).expect("pending map and diff are out of sync"))
    }

    pub fn get_head(&self, id: &SubstateId) -> Option<&SubstateChange> {
        self.head
            .get(id)
            .map(|&pos| self.changes.get(pos).expect("diff and head are not in sync"))
    }

    fn get_head_mut(&mut self, id: &SubstateId) -> Option<&mut SubstateChange> {
        let pos = *self.head.get(id)?;
        let change = self.changes.get_mut(pos).expect("diff and head are not in sync");
        if let Some(undo) = &mut self.undo &&
            pos < undo.num_changes
        {
            undo.rewritten.push((pos, change.clone()));
        }
        Some(change)
    }

    fn insert(&mut self, change: SubstateChange) {
        let index = self.changes.len();
        let address = change.to_substate_address();
        let id = change.versioned_substate_id().substate_id().clone();
        let prev_pending = self.pending.insert(address, index);
        let prev_head = self.head.insert(id.clone(), index);
        if let Some(undo) = &mut self.undo {
            undo.pending.push((address, prev_pending));
            undo.head.push((id, prev_head));
        }
        self.changes.push(change)
    }

    /// Restores the log to what it was when `undo` began recording.
    fn rollback(&mut self, undo: UndoLog) {
        self.changes.truncate(undo.num_changes);
        for (pos, change) in undo.rewritten.into_iter().rev() {
            self.changes[pos] = change;
        }
        for (address, prev) in undo.pending.into_iter().rev() {
            match prev {
                Some(index) => self.pending.insert(address, index),
                None => self.pending.remove(&address),
            };
        }
        for (id, prev) in undo.head.into_iter().rev() {
            match prev {
                Some(index) => self.head.insert(id, index),
                None => self.head.remove(&id),
            };
        }
    }

    pub fn get<B: SubstateLayer>(
        &self,
        base: &B,
        id: VersionedSubstateIdRef<'_>,
    ) -> Result<Substate, SubstateStoreError> {
        if let Some(change) = self.get_pending(&id.to_substate_address()) {
            return change
                .up_substate()
                .cloned()
                .ok_or_else(|| SubstateStoreError::SubstateNotFound {
                    id: change.versioned_substate_id().to_owned(),
                });
        }
        base.get_substate(id)
    }

    pub fn get_many<B, I>(&self, base: &B, ids: I) -> Result<HashMap<SubstateRequirement, Substate>, SubstateStoreError>
    where
        B: SubstateLayer,
        I: IntoIterator<Item = (SubstateRequirement, SubstateVersion)> + ExactSizeIterator,
    {
        let mut substates = HashMap::with_capacity(ids.len());
        for (req, version) in ids {
            let substate = self.get(base, VersionedSubstateIdRef::new(req.substate_id(), version))?;
            substates.insert(req, substate);
        }

        Ok(substates)
    }

    pub fn is_version_up<B: SubstateLayer>(
        &self,
        base: &B,
        id: VersionedSubstateIdRef<'_>,
    ) -> Result<Option<bool>, SubstateStoreError> {
        if let Some(change) = self.get_pending(&id.to_substate_address()) {
            return Ok(Some(change.is_up()));
        }
        base.is_version_up(id)
    }

    pub fn latest_change<B: SubstateLayer>(
        &self,
        base: &B,
        id: &SubstateId,
    ) -> Result<SubstateChange, SubstateStoreError> {
        if let Some(change) = self.get_head(id) {
            return Ok(change.clone());
        }
        base.latest_change(id)
    }

    pub fn latest_version<B: SubstateLayer>(
        &self,
        base: &B,
        id: &SubstateId,
    ) -> Result<LatestSubstateVersion, SubstateStoreError> {
        if let Some(change) = self.get_head(id) {
            return Ok(LatestSubstateVersion::new(
                change.versioned_substate_id().version(),
                change.is_up(),
            ));
        }
        base.latest_version(id)
    }

    pub fn assert_is_up<B: SubstateLayer>(
        &self,
        base: &B,
        id: VersionedSubstateIdRef<'_>,
    ) -> Result<(), SubstateStoreError> {
        trace!(
            target: LOG_TARGET,
            "assert_is_up: id: {}, pending: {}, head: {}",
            id,
            self.pending.display(),
            self.head.display()
        );
        match self.is_version_up(base, id)? {
            Some(true) => Ok(()),
            Some(false) | None => Err(SubstateStoreError::SubstateNotFound { id: id.to_owned() }),
        }
    }

    pub fn lock_assert_is_up<B: SubstateLayer>(
        &self,
        base: &B,
        id: VersionedSubstateIdRef<'_>,
    ) -> Result<(), SubstateStoreError> {
        match self.assert_is_up(base, id) {
            Ok(_) => Ok(()),
            // Converts a substate store error to a LockFailedError (TODO: improve)
            Err(SubstateStoreError::SubstateNotFound { id }) => Err(LockFailedError::SubstateNotFound { id }.into()),
            Err(err) => Err(err),
        }
    }

    pub fn assert_is_down<B: SubstateLayer>(
        &self,
        base: &B,
        id: VersionedSubstateIdRef<'_>,
    ) -> Result<(), SubstateStoreError> {
        match self.is_version_up(base, id)? {
            Some(false) => Ok(()),
            Some(true) => Err(SubstateStoreError::ExpectedSubstateDown { id: id.to_owned() }),
            None => {
                debug!(target: LOG_TARGET, "Expected substate {} to be DOWN but it does not exist", id);
                Err(SubstateStoreError::SubstateNotFound { id: id.to_owned() })
            },
        }
    }

    /// Records `change` once the version it replaces is DOWN (an UP) or the version it spends is UP (a DOWN).
    pub fn put<B: SubstateLayer>(&mut self, base: &B, change: SubstateChange) -> Result<(), SubstateStoreError> {
        match &change {
            SubstateChange::Up { id, substate, .. } => {
                if let Some(v) = substate.previous_version() {
                    self.assert_is_down(base, VersionedSubstateIdRef::new(id, v))?;
                }
            },
            SubstateChange::Down { id, .. } => {
                self.assert_is_up(base, id.as_versioned_ref())?;
            },
        }

        self.insert(change);
        Ok(())
    }

    /// Puts every DOWN, then every UP, then every validator fee withdrawal in `diff`, then calls `on_put` with each
    /// DOWN and UP. A failure part way leaves `self` unchanged and does not call `on_put`.
    pub fn put_diff<B, F>(&mut self, base: &B, diff: &SubstateDiff, mut on_put: F) -> Result<(), SubstateStoreError>
    where
        B: SubstateLayer,
        F: FnMut(&SubstateChange),
    {
        let num_changes = self.changes.len();
        self.undo = Some(UndoLog {
            num_changes,
            ..UndoLog::default()
        });
        let result = self.put_diff_changes(base, diff);
        let undo = self.undo.take().expect("set above");
        match result {
            Ok(num_puts) => {
                for change in &self.changes[num_changes..num_changes + num_puts] {
                    on_put(change);
                }
                Ok(())
            },
            Err(err) => {
                self.rollback(undo);
                Err(err)
            },
        }
    }

    /// The changes of [`Self::put_diff`]. Returns the number of DOWNs and UPs, which precede the withdrawals.
    fn put_diff_changes<B: SubstateLayer>(
        &mut self,
        base: &B,
        diff: &SubstateDiff,
    ) -> Result<usize, SubstateStoreError> {
        let num_changes = self.changes.len();
        let num_preshards = base.num_preshards();
        for (id, version) in diff.down_iter() {
            let id = VersionedSubstateId::new(id.clone(), *version);
            let shard = id.to_shard(num_preshards);
            debug!(target: LOG_TARGET, "🔽️ Down: {id} {shard}");
            self.put(base, SubstateChange::Down { id, shard })?;
        }

        for (id, substate) in diff.up_iter() {
            let vid = VersionedSubstateIdRef::new(id, substate.version());
            let shard = vid.to_shard(num_preshards);
            debug!(target: LOG_TARGET, "🔼️ Up: {} v{} {}", id, substate.version(), shard);
            self.put(base, SubstateChange::Up {
                id: id.clone(),
                shard,
                substate: Box::new(substate.clone()),
            })?;
        }

        let num_puts = self.changes.len() - num_changes;

        for withdraw in diff.validator_fee_withdrawals() {
            let id = withdraw.address.into();
            self.update_in_place(
                base,
                &id,
                |value_mut| {
                    let substate_type = SubstateType::from(&*value_mut);
                    let fee_pool =
                        value_mut
                            .as_validator_fee_pool_mut()
                            .ok_or_else(|| SubstateStoreError::InvariantError {
                                details: format!(
                                    "Expected substate {id} to be a ValidatorFeePool but was {substate_type}",
                                ),
                            })?;
                    if !fee_pool.withdraw_direct(withdraw.amount) {
                        return Err(SubstateStoreError::InvariantError {
                            details: format!(
                                "Insufficient balance to withdraw {} from validator fee pool {} (balance: {})",
                                withdraw.amount,
                                withdraw.address,
                                fee_pool.amount()
                            ),
                        });
                    }
                    Ok(())
                },
                // Without a live version there is nothing to withdraw from
                |maybe_id| match maybe_id {
                    Some(id) => Err(SubstateStoreError::SubstateNotFound { id: id.to_owned() }),
                    None => Err(SubstateStoreError::SubstateNotFound {
                        id: VersionedSubstateId::new(id.clone(), SubstateVersion::ZERO),
                    }),
                },
            )?;
        }

        Ok(num_puts)
    }

    /// Applies `updater` to the latest value of `substate_id`, or `creator` if it is DOWN or has never existed.
    ///
    /// A substate the block being built has already changed keeps that change's version: an UP is rewritten in place
    /// and a DOWN is followed by the next version. Otherwise the latest version is put DOWN and its successor UP.
    pub fn update_in_place<B, TErr, FUpdate, FCreate>(
        &mut self,
        base: &B,
        substate_id: &SubstateId,
        updater: FUpdate,
        creator: FCreate,
    ) -> Result<(), TErr>
    where
        B: SubstateLayer,
        TErr: From<SubstateStoreError>,
        FUpdate: FnOnce(&mut SubstateValue) -> Result<(), TErr>,
        FCreate: FnOnce(Option<VersionedSubstateIdRef<'_>>) -> Result<SubstateValue, TErr>,
    {
        let num_preshards = base.num_preshards();
        if let Some(head_change_mut) = self.get_head_mut(substate_id) {
            match head_change_mut {
                SubstateChange::Up { substate, .. } => {
                    debug!(target: LOG_TARGET, "Updating substate in place: {} v{}", substate_id, substate.version());
                    return updater(substate.substate_value_mut());
                },
                SubstateChange::Down { id, .. } => {
                    let id = id.clone();
                    let next_version = id
                        .version()
                        .checked_next()
                        .ok_or_else(|| SubstateStoreError::InvariantError {
                            details: format!("Substate {id} cannot be incremented"),
                        })?;
                    let value = creator(Some(id.as_versioned_ref()))?;
                    let next_id = VersionedSubstateId::new(id.into_substate_id(), next_version);
                    debug!(target: LOG_TARGET, "Creating substate in place: {}", next_id);
                    self.insert_next_up(next_id, value, num_preshards);
                },
            }
            return Ok(());
        }

        if let Some(head_change) = base.block_head_change(substate_id) {
            let mut head_change = head_change.clone();
            match &mut head_change {
                SubstateChange::Up { substate, .. } => {
                    debug!(target: LOG_TARGET, "Updating substate in place: {} v{}", substate_id, substate.version());
                    updater(substate.substate_value_mut())?;
                    self.insert(head_change);
                },
                SubstateChange::Down { id, .. } => {
                    let next_version = id
                        .version()
                        .checked_next()
                        .ok_or_else(|| SubstateStoreError::InvariantError {
                            details: format!("Substate {id} cannot be incremented"),
                        })?;
                    let value = creator(Some(id.as_versioned_ref()))?;
                    let next_id = VersionedSubstateId::new(id.clone().into_substate_id(), next_version);
                    debug!(target: LOG_TARGET, "Creating substate in place: {}", next_id);
                    self.insert_next_up(next_id, value, num_preshards);
                },
            }
            return Ok(());
        }

        let Some(change) = base.latest_change(substate_id).optional()? else {
            debug!(target: LOG_TARGET, "Creating substate in place: {} v0", substate_id);
            let value = creator(None)?;
            self.insert_next_up(
                VersionedSubstateId::new(substate_id.clone(), SubstateVersion::ZERO),
                value,
                num_preshards,
            );
            return Ok(());
        };
        match change {
            SubstateChange::Up {
                substate, id, shard, ..
            } => {
                let version = substate.version();
                let next_version = version
                    .checked_next()
                    .ok_or_else(|| SubstateStoreError::InvariantError {
                        details: format!("Substate {id} version {version} cannot be incremented"),
                    })?;
                let mut substate_value = substate.into_substate_value();
                debug!(
                    target: LOG_TARGET,
                    "Updating substate in place: {} v{} -> v{}",
                    id,
                    version,
                    next_version
                );
                updater(&mut substate_value)?;
                self.insert(SubstateChange::Down {
                    id: VersionedSubstateId::new(id.clone(), version),
                    shard,
                });
                let substate = Substate::new(next_version, substate_value);
                self.insert(SubstateChange::Up {
                    id,
                    shard,
                    substate: Box::new(substate),
                });
            },
            SubstateChange::Down { id, .. } => {
                debug!(target: LOG_TARGET, "Re-creating DOWNed substate in place: {}", id);
                let value = creator(Some(id.as_versioned_ref()))?;
                let next_version = id
                    .version()
                    .checked_next()
                    .ok_or_else(|| SubstateStoreError::InvariantError {
                        details: format!("Substate {id} cannot be incremented"),
                    })?;
                self.insert_next_up(
                    VersionedSubstateId::new(id.into_substate_id(), next_version),
                    value,
                    num_preshards,
                );
            },
        }
        Ok(())
    }

    fn insert_next_up(&mut self, id: VersionedSubstateId, value: SubstateValue, num_preshards: NumPreshards) {
        let up = SubstateChange::Up {
            shard: id.to_shard(num_preshards),
            substate: Box::new(Substate::new(id.version(), value)),
            id: id.into_substate_id(),
        };
        self.insert(up);
    }
}

#[cfg(test)]
mod tests {
    use tari_engine_types::{ValidatorFeePool, substate::SubstateValue};
    use tari_ootle_common_types::SubstateVersion;
    use tari_template_lib_types::ValidatorFeePoolAddress;

    use super::*;

    struct SingleUpLayer {
        id: SubstateId,
        substate: Substate,
    }

    impl SubstateLayer for SingleUpLayer {
        fn num_preshards(&self) -> NumPreshards {
            NumPreshards::P256
        }

        fn get_substate(&self, id: VersionedSubstateIdRef<'_>) -> Result<Substate, SubstateStoreError> {
            if id.substate_id() == &self.id && id.version() == self.substate.version() {
                Ok(self.substate.clone())
            } else {
                Err(SubstateStoreError::SubstateNotFound { id: id.to_owned() })
            }
        }

        fn is_version_up(&self, id: VersionedSubstateIdRef<'_>) -> Result<Option<bool>, SubstateStoreError> {
            if id.substate_id() == &self.id && id.version() == self.substate.version() {
                Ok(Some(true))
            } else {
                Ok(None)
            }
        }

        fn latest_change(&self, id: &SubstateId) -> Result<SubstateChange, SubstateStoreError> {
            if id == &self.id {
                let vid = VersionedSubstateIdRef::new(&self.id, self.substate.version());
                Ok(SubstateChange::Up {
                    id: self.id.clone(),
                    shard: vid.to_shard(NumPreshards::P256),
                    substate: Box::new(self.substate.clone()),
                })
            } else {
                Err(SubstateStoreError::SubstateNotFound {
                    id: VersionedSubstateId::new(id.clone(), SubstateVersion::ZERO),
                })
            }
        }

        fn latest_version(&self, id: &SubstateId) -> Result<LatestSubstateVersion, SubstateStoreError> {
            if id == &self.id {
                Ok(LatestSubstateVersion::new(self.substate.version(), true))
            } else {
                Err(SubstateStoreError::SubstateNotFound {
                    id: VersionedSubstateId::new(id.clone(), SubstateVersion::ZERO),
                })
            }
        }

        fn block_head_change(&self, _id: &SubstateId) -> Option<&SubstateChange> {
            None
        }
    }

    #[test]
    fn failed_update_in_place_does_not_record_orphan_down_change() {
        let pool_addr = ValidatorFeePoolAddress::from_array([7; 32]);
        let pool_id = SubstateId::ValidatorFeePool(pool_addr);
        let base = SingleUpLayer {
            id: pool_id.clone(),
            substate: Substate::new(
                SubstateVersion::ZERO,
                SubstateValue::ValidatorFeePool(ValidatorFeePool::new([7; 32].into(), 100)),
            ),
        };
        let mut log = ChangeLog::new();
        let err = log
            .update_in_place(
                &base,
                &pool_id,
                |_| {
                    Err::<(), _>(SubstateStoreError::InvariantError {
                        details: "updater failed".to_string(),
                    })
                },
                |_| unreachable!(),
            )
            .unwrap_err();
        assert!(matches!(err, SubstateStoreError::InvariantError { .. }));
        assert!(log.changes().is_empty(), "failed update_in_place must not leave a Down change");
        let still_up = log
            .get(&base, VersionedSubstateIdRef::new(&pool_id, SubstateVersion::ZERO))
            .unwrap();
        assert_eq!(still_up.substate_value().as_validator_fee_pool().unwrap().amount(), 100);
    }

    #[test]
    fn update_in_place_over_max_version_down_returns_invariant_error_instead_of_panicking() {
        let pool_addr = ValidatorFeePoolAddress::from_array([9; 32]);
        let pool_id = SubstateId::ValidatorFeePool(pool_addr);
        let max_ver = SubstateVersion::from(u32::MAX);
        let base = SingleUpLayer {
            id: pool_id.clone(),
            substate: Substate::new(
                max_ver,
                SubstateValue::ValidatorFeePool(ValidatorFeePool::new([9; 32].into(), 50)),
            ),
        };
        let mut log = ChangeLog::new();
        let vid = VersionedSubstateId::new(pool_id.clone(), max_ver);
        log.put(&base, SubstateChange::Down {
            shard: vid.to_shard(NumPreshards::P256),
            id: vid,
        })
        .unwrap();

        let err = log
            .update_in_place(
                &base,
                &pool_id,
                |_| unreachable!(),
                |_| {
                    Ok::<_, SubstateStoreError>(SubstateValue::ValidatorFeePool(ValidatorFeePool::new(
                        [9; 32].into(),
                        50,
                    )))
                },
            )
            .unwrap_err();
        assert!(matches!(err, SubstateStoreError::InvariantError { .. }));
    }
}
