//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display};

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_consensus_types::LeafBlock;
use tari_engine_types::{
    SubstateVersion,
    substate::{SubstateId, SubstateValue},
};
use tari_ootle_common_types::{
    LockIntent,
    SubstateAddress,
    SubstateLockType,
    SubstateRequirementRef,
    ToSubstateAddress,
};
use tari_ootle_transaction::TransactionId;

use crate::{StateStoreReadTransaction, StorageError, consensus_models::RequireLockIntentRef};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode, CborLen)]
pub struct SubstateLock {
    #[n(0)]
    lock_type: SubstateLockType,
    #[n(1)]
    transaction_id: TransactionId,
    #[n(2)]
    version: SubstateVersion,
    #[n(3)]
    is_local_only: bool,
}

impl SubstateLock {
    pub fn new(
        transaction_id: TransactionId,
        version: SubstateVersion,
        lock_type: SubstateLockType,
        is_local_only: bool,
    ) -> Self {
        Self {
            transaction_id,
            version,
            lock_type,
            is_local_only,
        }
    }

    pub fn transaction_id(&self) -> &TransactionId {
        &self.transaction_id
    }

    pub fn lock_type(&self) -> SubstateLockType {
        self.lock_type
    }

    pub fn version(&self) -> SubstateVersion {
        self.version
    }

    pub fn is_local_only(&self) -> bool {
        self.is_local_only
    }

    pub fn is_write(&self) -> bool {
        self.lock_type.is_write()
    }

    pub fn is_read(&self) -> bool {
        self.lock_type.is_read()
    }

    pub fn is_input(&self) -> bool {
        self.lock_type().is_input()
    }

    pub fn is_output(&self) -> bool {
        self.lock_type().is_output()
    }
}

impl fmt::Display for SubstateLock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SubstateLock(version: {}, lock_flag: {}, is_local_only: {}, transaction_id: {})",
            self.version, self.lock_type, self.is_local_only, self.transaction_id,
        )
    }
}

#[derive(Debug, Clone)]
pub struct LockedSubstateValue {
    pub substate_id: SubstateId,
    pub lock: SubstateLock,
    /// The value of the locked substate. This may be None if the substate lock is Output.
    pub value: Option<SubstateValue>,
}

impl LockedSubstateValue {
    pub(crate) fn to_substate_lock_intent(&self) -> RequireLockIntentRef<'_> {
        RequireLockIntentRef::new(&self.substate_id, self.lock.version(), self.lock.lock_type())
    }

    pub fn substate_id(&self) -> &SubstateId {
        &self.substate_id
    }

    pub fn satisfies_requirements<'a, T: Into<SubstateRequirementRef<'a>>>(&self, requirement: T) -> bool {
        let requirement = requirement.into();
        self.lock.is_input() &&
            requirement.version().is_none_or(|v| v == self.lock.version()) &&
            *requirement.substate_id() == self.substate_id
    }

    pub fn satisfies_lock_intent<T: LockIntent>(&self, lock_intent: T) -> bool {
        lock_intent.version_to_lock() == self.lock.version() &&
            self.lock.lock_type().allows(lock_intent.lock_type()) &&
            *lock_intent.substate_id() == self.substate_id
    }

    pub fn take_value(&mut self) -> Option<SubstateValue> {
        self.value.take()
    }
}

impl LockedSubstateValue {
    /// The locks the chain ending at `leaf_block` holds for a transaction.
    pub fn get_all_for_transaction<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        leaf_block: &LeafBlock,
        transaction_id: &TransactionId,
    ) -> Result<Vec<LockedSubstateValue>, StorageError> {
        tx.substate_locks_get_locked_substates_for_transaction(leaf_block, transaction_id)
    }

    /// A transaction other than `exclude_transaction_id` that the chain ending at `leaf_block` has given a write lock
    /// over any of `substate_ids`.
    pub fn get_transaction_id_that_conflicts_with_write_locks<'a, TTx, I>(
        tx: &TTx,
        leaf_block: &LeafBlock,
        exclude_transaction_id: &TransactionId,
        substate_ids: I,
    ) -> Result<Option<TransactionId>, StorageError>
    where
        TTx: StateStoreReadTransaction,
        I: IntoIterator<Item = &'a SubstateId>,
    {
        tx.substate_locks_has_any_write_locks_for_substates(leaf_block, Some(exclude_transaction_id), substate_ids)
    }
}

impl ToSubstateAddress for LockedSubstateValue {
    fn to_substate_address(&self) -> SubstateAddress {
        SubstateAddress::from_substate_id(&self.substate_id, self.lock.version())
    }
}

impl Display for LockedSubstateValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "LockedSubstate(substate_id: {}, lock: {})",
            self.substate_id, self.lock,
        )
    }
}

#[cfg(test)]
mod tests {
    use tari_template_lib::types::{ComponentAddress, ObjectKey};

    use super::*;

    #[test]
    fn output_lock_does_not_satisfy_input_requirement() {
        let substate_id = SubstateId::Component(ComponentAddress::new(ObjectKey::from_array([7u8; ObjectKey::LENGTH])));
        let version = SubstateVersion::new(1);
        let versioned_req = SubstateRequirementRef::new(&substate_id, Some(version));
        let unversioned_req = SubstateRequirementRef::new(&substate_id, None);

        let output_lock = LockedSubstateValue {
            substate_id: substate_id.clone(),
            lock: SubstateLock::new(TransactionId::default(), version, SubstateLockType::Output, false),
            value: None,
        };
        assert!(!output_lock.satisfies_requirements(versioned_req));
        assert!(!output_lock.satisfies_requirements(unversioned_req));

        for lock_type in [SubstateLockType::Read, SubstateLockType::Write] {
            let input_lock = LockedSubstateValue {
                substate_id: substate_id.clone(),
                lock: SubstateLock::new(TransactionId::default(), version, lock_type, false),
                value: None,
            };
            assert!(input_lock.satisfies_requirements(versioned_req));
            assert!(input_lock.satisfies_requirements(unversioned_req));
        }
    }
}
