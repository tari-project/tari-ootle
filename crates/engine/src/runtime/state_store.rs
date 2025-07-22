//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, mem};

use indexmap::{IndexMap, IndexSet};
use tari_engine_types::{
    component::ComponentHeader,
    lock::{LockFlag, LockId},
    substate::{Substate, SubstateId, SubstateValue},
    vault::Vault,
    Utxo,
    UtxoAddress,
};
use tari_ootle_common_types::optional::Optional;
use tari_template_lib::models::{ComponentAddress, VaultId};

use crate::{
    runtime::{
        locking::{LockError, LockedSubstates},
        RuntimeError,
    },
    state_store::{memory::ReadOnlyMemoryStateStore, StateReader},
};

#[derive(Debug, Clone)]
pub struct WorkingStateStore {
    // This must be ordered deterministically since we use this to create the substate diff
    /// New and mutates substates are placed into this map.
    new_substates: IndexMap<SubstateId, SubstateValue>,

    /// When substates are locked, the ids and values are placed into this map.
    loaded_substates: HashMap<SubstateId, Substate>,
    /// Tracks the locked substate IDs and their lock states.
    locked_substates: LockedSubstates,

    downed_utxos: IndexSet<UtxoAddress>,
    /// The underlying state store that is used to load substates that are not in the working state maps.
    state_store: ReadOnlyMemoryStateStore,
}

impl WorkingStateStore {
    pub fn new(state_store: ReadOnlyMemoryStateStore) -> Self {
        Self {
            new_substates: IndexMap::new(),
            loaded_substates: HashMap::new(),
            locked_substates: LockedSubstates::default(),
            downed_utxos: IndexSet::default(),
            state_store,
        }
    }

    pub fn try_lock(&mut self, id: &SubstateId, lock_flag: LockFlag) -> Result<LockId, RuntimeError> {
        if !self.exists(id)? {
            return Err(RuntimeError::SubstateNotFound { id: id.clone() });
        }
        let lock_id = self.locked_substates.try_lock(id, lock_flag)?;
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
        let substate = self.get_for_mut(lock.substate_id())?;
        Ok((lock.substate_id().clone(), substate))
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
            match callback(lock.substate_id(), substate.substate_value_mut())? {
                Some(ret) => {
                    self.new_substates
                        .insert(lock.substate_id().clone(), substate.into_substate_value());
                    return Ok(Some(ret));
                },
                None => {
                    // It is undefined (i.e. a bug) to mutate the state and return None from the callback.
                    // We do not explicitly assert this however for performance reasons.
                    self.loaded_substates.insert(lock.substate_id().clone(), substate);
                    return Ok(None);
                },
            }
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

    pub fn exists(&self, id: &SubstateId) -> Result<bool, RuntimeError> {
        let exists = self.new_substates.contains_key(id) ||
            self.loaded_substates.contains_key(id) ||
            self.state_store.exists(id)?;
        Ok(exists)
    }

    pub fn insert(&mut self, id: SubstateId, value: SubstateValue) -> Result<(), RuntimeError> {
        if self.exists(&id)? {
            return Err(RuntimeError::DuplicateSubstate { address: id });
        }
        self.new_substates.insert(id, value);
        Ok(())
    }

    fn load_and_cache(&mut self, id: &SubstateId) -> Result<(), RuntimeError> {
        if self.new_substates.contains_key(id) {
            return Ok(());
        }
        if self.loaded_substates.contains_key(id) {
            return Ok(());
        }
        let substate = self
            .state_store
            .get_state(id)
            .optional()?
            .ok_or_else(|| RuntimeError::SubstateNotFound { id: id.clone() })?;
        self.loaded_substates.insert(id.clone(), substate.clone());
        Ok(())
    }

    pub fn take_mutated_substates(&mut self) -> IndexMap<SubstateId, SubstateValue> {
        mem::take(&mut self.new_substates)
    }

    pub fn take_downed_utxos(&mut self) -> IndexSet<UtxoAddress> {
        mem::take(&mut self.downed_utxos)
    }

    pub fn mutated_substates(&self) -> &IndexMap<SubstateId, SubstateValue> {
        &self.new_substates
    }

    pub fn down_utxo(&mut self, lock_id: LockId) -> Result<Utxo, RuntimeError> {
        let lock = self.locked_substates.get(lock_id, LockFlag::Write)?;
        let substate_id = lock.substate_id();
        let address = substate_id
            .as_utxo_address()
            .ok_or_else(|| RuntimeError::InvariantError {
                function: "down_utxo",
                details: format!("Substate at address {} is not a UTXO", substate_id),
            })?;
        const EXPECT: &str = "invariant: substate found at utxo address is not a UTXO";
        if let Some(value) = self.new_substates.shift_remove(substate_id) {
            self.downed_utxos.insert(address);
            return Ok(value.into_utxo().expect(EXPECT));
        }
        if let Some(substate) = self.loaded_substates.remove(substate_id) {
            self.downed_utxos.insert(address);
            return Ok(substate.into_substate_value().into_utxo().expect(EXPECT));
        }

        Err(RuntimeError::SubstateNotFound {
            id: substate_id.clone(),
        })
    }

    pub fn new_vaults(&self) -> impl Iterator<Item = (VaultId, &Vault)> + '_ {
        self.new_substates
            .iter()
            .filter(|(address, _)| address.is_vault())
            .map(|(addr, vault)| (addr.as_vault_id().unwrap(), vault.as_vault().unwrap()))
    }

    pub(super) fn state_store(&self) -> &ReadOnlyMemoryStateStore {
        &self.state_store
    }

    /// Loads and caches the component address. No lock is required for this operation.
    pub fn load_and_cache_component(&mut self, address: &ComponentAddress) -> Result<&ComponentHeader, RuntimeError> {
        let addr = SubstateId::Component(*address);
        self.load_and_cache(&addr)?;
        let component = self.get_ref(&addr)?;
        component.component().ok_or_else(|| RuntimeError::InvariantError {
            function: "load_component",
            details: format!("Substate at address {} is not a component", addr),
        })
    }

    pub(super) fn get_unmodified_substate(&self, address: &SubstateId) -> Result<&Substate, RuntimeError> {
        match self.loaded_substates.get(address) {
            Some(substate) => Ok(substate),
            None => self
                .state_store
                .get_state(address)
                .optional()?
                .ok_or_else(|| RuntimeError::SubstateNotFound { id: address.clone() }),
        }
    }
}
