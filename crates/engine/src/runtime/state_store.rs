//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    mem,
};

use indexmap::{IndexMap, IndexSet};
use tari_engine_types::{
    Utxo,
    component::Component,
    confidential_output::ConfidentialOutput,
    lock::{LockFlag, LockId},
    substate::{Substate, SubstateId, SubstateValue},
    vault::Vault,
};
use tari_ootle_common_types::optional::Optional;
use tari_template_lib::types::{ComponentAddress, VaultId};

use crate::{
    runtime::{
        RuntimeError,
        locking::{LockError, LockedSubstates},
    },
    state_store::StateReader,
};

#[derive(Debug, Clone)]
pub struct WorkingStateStore<TStore> {
    // This must be ordered deterministically since we use this to create the substate diff
    /// New and mutates substates are placed into this map.
    new_substates: IndexMap<SubstateId, SubstateValue>,

    /// When substates are locked, the ids and values are placed into this map.
    loaded_substates: HashMap<SubstateId, Substate>,
    /// Tracks the locked substate IDs and their lock states.
    locked_substates: LockedSubstates,

    /// Substates this transaction removed from state without writing a later version, in the order they were downed.
    downed: IndexSet<SubstateId>,
    /// Inputs the transaction declared as reads. A shard group that does not hold an input locks it
    /// from that declaration without executing, so by the time execution runs, a read declaration is
    /// a promise the rest of the network has already acted on: the transaction produces no new
    /// version of it, does not remove it, and records no change for consensus to apply to it.
    ///
    /// A write lock is an intra-transaction exclusivity claim taken for every `&mut self` call, and
    /// a call that leaves its component unchanged is rolled back, as a fee payment through an
    /// account is. So a read-declared input may be write-locked, and is refused where a mutation is
    /// kept.
    read_declared_inputs: HashSet<SubstateId>,
    /// The underlying state store that is used to load substates that are not in the working state maps.
    state_store: TStore,
}

impl<TStore: StateReader> WorkingStateStore<TStore> {
    pub fn new(state_store: TStore, read_declared_inputs: HashSet<SubstateId>) -> Self {
        Self {
            new_substates: IndexMap::new(),
            loaded_substates: HashMap::new(),
            locked_substates: LockedSubstates::default(),
            downed: IndexSet::default(),
            read_declared_inputs,
            state_store,
        }
    }

    pub fn try_lock(&mut self, id: SubstateId, lock_flag: LockFlag) -> Result<LockId, RuntimeError> {
        if self.is_spent(&id) {
            return Err(RuntimeError::SubstateAlreadySpent { id });
        }
        if !self.exists(&id)? {
            return Err(RuntimeError::SubstateNotFound { id: id.clone() });
        }
        let lock_id = self.locked_substates.try_lock(id.clone(), lock_flag)?;
        self.load_and_cache(id)?;
        Ok(lock_id)
    }

    pub fn try_unlock(&mut self, lock_id: LockId) -> Result<(), LockError> {
        self.locked_substates.try_unlock(lock_id)?;
        Ok(())
    }

    pub fn get_locked_substate_mut(
        &mut self,
        lock_id: LockId,
    ) -> Result<(SubstateId, &mut SubstateValue), RuntimeError> {
        let lock = self.locked_substates.get(lock_id, LockFlag::Write)?;
        self.ensure_writable(lock.substate_id())?;
        let substate = self.get_for_mut(lock.substate_id())?;
        Ok((lock.substate_id().clone(), substate))
    }

    /// Refuses to keep a mutation of an input the transaction declared as a read. See
    /// `read_declared_inputs`.
    fn ensure_writable(&self, id: &SubstateId) -> Result<(), RuntimeError> {
        if self.read_declared_inputs.contains(id) {
            return Err(RuntimeError::WriteToReadDeclaredInput { id: id.clone() });
        }
        Ok(())
    }

    pub fn mutate_locked_substate_with<
        R,
        F: FnOnce(&SubstateId, &mut SubstateValue) -> Result<Option<R>, RuntimeError>,
    >(
        &mut self,
        lock_id: LockId,
        callback: F,
    ) -> Result<Option<R>, RuntimeError> {
        let lock = self.locked_substates.get(lock_id, LockFlag::Write)?;
        if let Some(mut substate) = self.loaded_substates.remove(lock.substate_id()) {
            if self.read_declared_inputs.contains(lock.substate_id()) {
                // A kept mutation is refused, and a rolled-back one must leave the loaded value as it
                // was, so the callback runs on a copy that is only ever discarded.
                let mut scratch = substate.clone();
                let kept = callback(lock.substate_id(), scratch.substate_value_mut());
                self.loaded_substates.insert(lock.substate_id().clone(), substate);
                return match kept? {
                    Some(_) => Err(RuntimeError::WriteToReadDeclaredInput {
                        id: lock.substate_id().clone(),
                    }),
                    None => Ok(None),
                };
            }
            return match callback(lock.substate_id(), substate.substate_value_mut())? {
                Some(ret) => {
                    self.new_substates
                        .insert(lock.substate_id().clone(), substate.into_substate_value());
                    Ok(Some(ret))
                },
                None => {
                    // It is undefined (i.e. a bug) to mutate the state and return None from the callback.
                    // We do not explicitly assert this however for performance reasons.
                    self.loaded_substates.insert(lock.substate_id().clone(), substate);
                    Ok(None)
                },
            };
        }

        let substate_mut =
            self.new_substates
                .get_mut(lock.substate_id())
                .ok_or_else(|| LockError::SubstateNotLocked {
                    address: lock.substate_id().clone(),
                })?;

        // Since the substate is already mutated, we don't really care if the callback mutates it again or not
        callback(lock.substate_id(), substate_mut)
    }

    pub fn get_locked_substate(&self, lock_id: LockId) -> Result<(SubstateId, &SubstateValue), RuntimeError> {
        let lock = self.locked_substates.get(lock_id, LockFlag::Read)?;
        let substate = self.get_ref(lock.substate_id())?;
        Ok((lock.substate_id().clone(), substate))
    }

    fn get_ref(&self, address: &SubstateId) -> Result<&SubstateValue, LockError> {
        self.new_substates
            .get(address)
            .or_else(|| self.loaded_substates.get(address).map(|s| s.substate_value()))
            .ok_or_else(|| LockError::SubstateNotLocked {
                address: address.clone(),
            })
    }

    fn get_for_mut(&mut self, address: &SubstateId) -> Result<&mut SubstateValue, LockError> {
        if let Some(substate) = self.loaded_substates.remove(address) {
            self.new_substates
                .insert(address.clone(), substate.into_substate_value());
        }

        if let Some(substate_mut) = self.new_substates.get_mut(address) {
            return Ok(substate_mut);
        }

        Err(LockError::SubstateNotLocked {
            address: address.clone(),
        })
    }

    /// Whether `id` names a substate this transaction has already downed. The backing store is untouched until the
    /// transaction commits, so `downed` is the whole record of what this transaction has removed; every presence and
    /// load path consults it so that a downed substate reads as gone for the rest of the transaction.
    fn is_spent(&self, id: &SubstateId) -> bool {
        self.downed.contains(id)
    }

    pub fn exists(&self, id: &SubstateId) -> Result<bool, RuntimeError> {
        if self.is_spent(id) {
            return Ok(false);
        }
        let exists = self.new_substates.contains_key(id) ||
            self.loaded_substates.contains_key(id) ||
            self.state_store.exists(id)?;
        Ok(exists)
    }

    pub fn insert(&mut self, id: SubstateId, value: SubstateValue) -> Result<(), RuntimeError> {
        // A spent address reads as absent to `exists`, so this is the check that keeps the substate diff to one
        // record per address: a down or an up, never both.
        if self.is_spent(&id) {
            return Err(RuntimeError::SubstateAlreadySpent { id });
        }
        if self.exists(&id)? {
            return Err(RuntimeError::SubstateAlreadyExists { address: id });
        }
        self.new_substates.insert(id, value);
        Ok(())
    }

    fn load_and_cache(&mut self, id: SubstateId) -> Result<&SubstateValue, RuntimeError> {
        if self.is_spent(&id) {
            return Err(RuntimeError::SubstateAlreadySpent { id });
        }
        if let Some(s) = self.new_substates.get(&id) {
            return Ok(s);
        }
        // Check if it exists first
        if !self.loaded_substates.contains_key(&id) {
            // If not, fetch it (this is where the mutation happens)
            let substate = self
                .state_store
                .get_state(&id)
                .optional()?
                .ok_or_else(|| RuntimeError::SubstateNotFound { id: id.clone() })?;

            self.loaded_substates.insert(id.clone(), substate.clone());
        }

        // unwrap: we just inserted it or confirmed it exists.
        Ok(self.loaded_substates.get(&id).unwrap().substate_value())
    }

    pub fn take_mutated_substates(&mut self) -> IndexMap<SubstateId, SubstateValue> {
        mem::take(&mut self.new_substates)
    }

    pub fn take_downed(&mut self) -> IndexSet<SubstateId> {
        mem::take(&mut self.downed)
    }

    /// The substates the transaction has created so far, excluding new versions of substates it was given.
    pub fn created_substate_ids(&self) -> Result<Vec<SubstateId>, RuntimeError> {
        let mut created = Vec::new();
        for id in self.new_substates.keys() {
            if !self.loaded_substates.contains_key(id) && !self.state_store.exists(id)? {
                created.push(id.clone());
            }
        }
        Ok(created)
    }

    pub fn mutated_substates(&self) -> &IndexMap<SubstateId, SubstateValue> {
        &self.new_substates
    }

    pub fn downed(&self) -> &IndexSet<SubstateId> {
        &self.downed
    }

    /// Mutates a write-locked substate for the rest of the transaction without adding it to the substates the
    /// transaction persists. For a substate whose writes consensus applies itself: a validator fee pool is credited
    /// and debited in place per block, so the engine records a withdrawal rather than a new version of the pool.
    pub fn mutate_unpersisted<R>(
        &mut self,
        lock_id: LockId,
        callback: impl FnOnce(&mut SubstateValue) -> Result<R, RuntimeError>,
    ) -> Result<R, RuntimeError> {
        let lock = self.locked_substates.get(lock_id, LockFlag::Write)?;
        self.ensure_writable(lock.substate_id())?;
        let substate =
            self.loaded_substates
                .get_mut(lock.substate_id())
                .ok_or_else(|| RuntimeError::InvariantError {
                    function: "mutate_unpersisted",
                    details: format!(
                        "Substate {} is not loaded, or is already set to persist",
                        lock.substate_id()
                    ),
                })?;
        callback(substate.substate_value_mut())
    }

    /// Removes the write-locked substate from state for the rest of the transaction and returns its value.
    ///
    /// `new_substates` holds mutated substates as well as created ones, so presence there does not mean the substate
    /// is new: one mutated earlier in this transaction (an unfrozen output, say) was moved there from
    /// `loaded_substates`. Only a substate that did not exist before this transaction may collapse without a down; a
    /// pre-existing one must still be downed, or it stays live after its value is gone.
    fn down(&mut self, lock_id: LockId) -> Result<SubstateValue, RuntimeError> {
        let lock = self.locked_substates.get(lock_id, LockFlag::Write)?;
        let substate_id = lock.substate_id().clone();
        self.ensure_writable(&substate_id)?;
        if let Some(value) = self.new_substates.shift_remove(&substate_id) {
            if self.get_unmodified_substate(&substate_id).optional()?.is_some() {
                self.downed.insert(substate_id);
            }
            return Ok(value);
        }
        if let Some(substate) = self.loaded_substates.remove(&substate_id) {
            self.downed.insert(substate_id);
            return Ok(substate.into_substate_value());
        }

        Err(RuntimeError::SubstateNotFound { id: substate_id })
    }

    pub fn down_utxo(&mut self, lock_id: LockId) -> Result<Utxo, RuntimeError> {
        let substate_id = self
            .locked_substates
            .get(lock_id, LockFlag::Write)?
            .substate_id()
            .clone();
        if substate_id.as_utxo_address().is_none() {
            return Err(RuntimeError::InvariantError {
                function: "down_utxo",
                details: format!("Substate at address {} is not a UTXO", substate_id),
            });
        }
        self.down(lock_id)?
            .into_utxo()
            .ok_or_else(|| RuntimeError::InvariantError {
                function: "down_utxo",
                details: format!("Substate at utxo address {} does not hold a UTXO", substate_id),
            })
    }

    pub fn down_confidential_output(&mut self, lock_id: LockId) -> Result<ConfidentialOutput, RuntimeError> {
        let substate_id = self
            .locked_substates
            .get(lock_id, LockFlag::Write)?
            .substate_id()
            .clone();
        if substate_id.as_confidential_output_address().is_none() {
            return Err(RuntimeError::InvariantError {
                function: "down_confidential_output",
                details: format!("Substate at address {} is not a confidential output", substate_id),
            });
        }
        self.down(lock_id)?
            .into_confidential_output()
            .ok_or_else(|| RuntimeError::InvariantError {
                function: "down_confidential_output",
                details: format!(
                    "Substate at confidential output address {} does not hold a confidential output",
                    substate_id
                ),
            })
    }

    /// The address kind is the filter; a value that disagrees with it is the invariant break `down_utxo`
    /// names, not an entry to skip. This feeds the dangling-locked-value check at commit, so skipping one
    /// would skip the check rather than report anything.
    pub fn new_vaults(&self) -> Result<Vec<(VaultId, &Vault)>, RuntimeError> {
        self.new_substates
            .iter()
            .filter_map(|(addr, value)| Some((addr.as_vault_id()?, value)))
            .map(|(vault_id, value)| {
                let vault = value.as_vault().ok_or_else(|| RuntimeError::InvariantError {
                    function: "new_vaults",
                    details: format!("Substate at vault address {} does not hold a vault", vault_id),
                })?;
                Ok((vault_id, vault))
            })
            .collect()
    }

    /// Loads and caches the component address. No lock is required for this operation.
    pub fn load_and_cache_component(&mut self, address: ComponentAddress) -> Result<&Component, RuntimeError> {
        let addr = SubstateId::Component(address);
        let component = self.load_and_cache(addr)?;
        component.component().ok_or_else(|| RuntimeError::InvariantError {
            function: "load_component",
            details: format!("Substate at address {} is not a component", address),
        })
    }

    pub(super) fn get_unmodified_substate(&self, id: &SubstateId) -> Result<&Substate, RuntimeError> {
        match self.loaded_substates.get(id) {
            Some(substate) => Ok(substate),
            None => self
                .state_store
                .get_state(id)
                .optional()?
                .ok_or_else(|| RuntimeError::SubstateNotFound { id: id.clone() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use core::slice;

    use tari_engine_types::confidential::ClaimedOutputTombstone;
    use tari_template_lib::types::{ClaimedOutputTombstoneAddress, ObjectKey};

    use super::*;
    use crate::state_store::memory::MemoryStateStore;

    fn tombstone_id(byte: u8) -> SubstateId {
        SubstateId::ClaimedOutputTombstone(ClaimedOutputTombstoneAddress::new(ObjectKey::from_array(
            [byte; ObjectKey::LENGTH],
        )))
    }

    fn store_holding(ids: &[SubstateId], read_declared: &[SubstateId]) -> WorkingStateStore<MemoryStateStore> {
        let mut backing = MemoryStateStore::new();
        backing
            .set_many(
                ids.iter()
                    .map(|id| (id.clone(), Substate::new(0u64, ClaimedOutputTombstone { value: 1 }))),
            )
            .unwrap();
        WorkingStateStore::new(backing, read_declared.iter().cloned().collect())
    }

    fn assert_write_to_read_declared(err: RuntimeError, id: &SubstateId) {
        assert!(
            matches!(&err, RuntimeError::WriteToReadDeclaredInput { id: got } if got == id),
            "{err}"
        );
    }

    /// A `&mut self` call takes a write lock whether or not it changes anything, and a call that
    /// changes nothing is rolled back rather than persisted, so the lock itself breaks no promise.
    #[test]
    fn a_read_declared_substate_takes_a_write_lock_and_rolls_back_a_no_op() {
        let id = tombstone_id(1);
        let mut store = store_holding(slice::from_ref(&id), slice::from_ref(&id));

        let lock_id = store.try_lock(id.clone(), LockFlag::Write).unwrap();
        let kept = store
            .mutate_locked_substate_with(lock_id, |_, _| Ok(None::<()>))
            .unwrap();
        assert!(kept.is_none());
        assert!(store.mutated_substates().is_empty());
    }

    #[test]
    fn a_read_declared_substate_refuses_a_kept_mutation() {
        let id = tombstone_id(4);
        let mut store = store_holding(slice::from_ref(&id), slice::from_ref(&id));

        let lock_id = store.try_lock(id.clone(), LockFlag::Write).unwrap();
        let err = store
            .mutate_locked_substate_with(lock_id, |_, value| {
                *value = ClaimedOutputTombstone { value: 2 }.into();
                Ok(Some(()))
            })
            .unwrap_err();
        assert_write_to_read_declared(err, &id);
        assert!(store.mutated_substates().is_empty());
        // The lock is still held, and what is loaded under it is the value before the refused call.
        let (_, loaded) = store.get_locked_substate(lock_id).unwrap();
        assert_eq!(loaded.as_claimed_output_tombstone().unwrap().value, 1);

        let err = store.get_locked_substate_mut(lock_id).unwrap_err();
        assert_write_to_read_declared(err, &id);
    }

    #[test]
    fn a_read_declared_substate_refuses_an_unpersisted_mutation() {
        let id = tombstone_id(5);
        let mut store = store_holding(slice::from_ref(&id), slice::from_ref(&id));

        let lock_id = store.try_lock(id.clone(), LockFlag::Write).unwrap();
        let err = store.mutate_unpersisted(lock_id, |_| Ok(())).unwrap_err();
        assert_write_to_read_declared(err, &id);
    }

    #[test]
    fn a_read_declared_substate_refuses_a_down() {
        let id = tombstone_id(6);
        let mut store = store_holding(slice::from_ref(&id), slice::from_ref(&id));

        let lock_id = store.try_lock(id.clone(), LockFlag::Write).unwrap();
        let err = store.down(lock_id).unwrap_err();
        assert_write_to_read_declared(err, &id);
        assert!(store.downed().is_empty());
    }

    #[test]
    fn a_read_declared_substate_still_takes_a_read_lock() {
        let id = tombstone_id(2);
        let mut store = store_holding(slice::from_ref(&id), slice::from_ref(&id));

        store.try_lock(id, LockFlag::Read).unwrap();
    }

    #[test]
    fn an_undeclared_substate_takes_a_write_lock() {
        let id = tombstone_id(3);
        let mut store = store_holding(slice::from_ref(&id), &[]);

        store.try_lock(id, LockFlag::Write).unwrap();
    }
}
