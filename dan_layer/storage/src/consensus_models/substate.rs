//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, fmt, fmt::Display};

use serde::{Deserialize, Serialize};
use tari_common_types::types::FixedHash;
use tari_dan_common_types::{
    displayable::Displayable,
    shard::Shard,
    Epoch,
    NodeHeight,
    SubstateAddress,
    SubstateRequirement,
    VersionedSubstateId,
    VersionedSubstateIdRef,
};
use tari_engine_types::{
    serde_with,
    substate::{hash_substate, Substate, SubstateId, SubstateValue},
};
use tari_transaction::TransactionId;

use crate::{
    consensus_models::{BlockId, QcId, QuorumCertificate, SubstateLock},
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct SubstateRecord {
    pub substate_id: SubstateId,
    pub version: u32,
    pub substate_value: Option<SubstateValue>,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[serde(with = "serde_with::hex")]
    pub state_hash: FixedHash,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub created_justify: QcId,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub created_block: BlockId,
    pub created_by_shard: Shard,
    pub created_at_epoch: Epoch,
    pub destroyed: Option<SubstateDestroyed>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct SubstateDestroyed {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub justify: QcId,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub by_block: NodeHeight,
    pub at_epoch: Epoch,
    pub by_shard: Shard,
}

impl SubstateRecord {
    pub fn new<V: Into<SubstateValueOrHash>>(
        substate_id: SubstateId,
        version: u32,
        value: V,
        created_by_shard: Shard,
        created_at_epoch: Epoch,
        created_block: BlockId,
        created_justify: QcId,
    ) -> Self {
        let value = value.into();
        Self {
            substate_id,
            version,
            state_hash: value.to_value_hash(version),
            substate_value: value.into_value(),
            created_justify,
            created_by_shard,
            created_at_epoch,
            created_block,
            destroyed: None,
        }
    }

    pub fn to_substate_address(&self) -> SubstateAddress {
        SubstateAddress::from_substate_id(&self.substate_id, self.version)
    }

    pub fn to_versioned_substate_id(&self) -> VersionedSubstateId {
        VersionedSubstateId::new(self.substate_id.clone(), self.version)
    }

    pub fn to_substate_requirement(&self) -> SubstateRequirement {
        SubstateRequirement::versioned(self.substate_id.clone(), self.version)
    }

    pub fn substate_id(&self) -> &SubstateId {
        &self.substate_id
    }

    pub fn substate_value(&self) -> Option<&SubstateValue> {
        self.substate_value.as_ref()
    }

    pub fn clear_substate_value(&mut self) {
        self.substate_value = None;
    }

    pub fn into_substate_value(self) -> Option<SubstateValue> {
        self.substate_value
    }

    pub fn into_substate(self) -> Option<Substate> {
        Some(Substate::new(self.version, self.substate_value?))
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn created_block(&self) -> BlockId {
        self.created_block
    }

    pub fn created_justify(&self) -> &QcId {
        &self.created_justify
    }

    pub fn destroyed(&self) -> Option<&SubstateDestroyed> {
        self.destroyed.as_ref()
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed.is_some()
    }

    pub fn is_up(&self) -> bool {
        !self.is_destroyed()
    }

    pub fn state_hash(&self) -> &FixedHash {
        &self.state_hash
    }
}

impl SubstateRecord {
    pub fn lock_all<
        'a,
        TTx: StateStoreWriteTransaction,
        I: IntoIterator<Item = (&'a SubstateId, &'a Vec<SubstateLock>)>,
    >(
        tx: &mut TTx,
        block_id: &BlockId,
        locks: I,
    ) -> Result<(), StorageError> {
        tx.substate_locks_insert_all(block_id, locks)
    }

    pub fn unlock_all<'a, TTx: StateStoreWriteTransaction, I: Iterator<Item = &'a TransactionId>>(
        tx: &mut TTx,
        transaction_ids: I,
    ) -> Result<(), StorageError> {
        tx.substate_locks_remove_many_for_transactions(transaction_ids)
    }

    pub fn create<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.substates_create(self)?;
        Ok(())
    }

    pub fn exists<TTx: StateStoreReadTransaction>(tx: &TTx, id: &VersionedSubstateId) -> Result<bool, StorageError> {
        Self::any_exist(tx, Some(id.as_ref()))
    }

    pub fn any_exist<'a, TTx: StateStoreReadTransaction, I: IntoIterator<Item = VersionedSubstateIdRef<'a>>>(
        tx: &TTx,
        substates: I,
    ) -> Result<bool, StorageError> {
        tx.substates_any_exist(substates)
    }

    pub fn get<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        address: &SubstateAddress,
    ) -> Result<SubstateRecord, StorageError> {
        tx.substates_get(address)
    }

    pub fn substate_is_up<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        shard: &SubstateAddress,
    ) -> Result<bool, StorageError> {
        // TODO: consider optimising
        let rec = tx.substates_get(shard)?;
        Ok(rec.is_up())
    }

    pub fn get_any<'a, TTx: StateStoreReadTransaction, I: IntoIterator<Item = VersionedSubstateIdRef<'a>>>(
        tx: &TTx,
        requirements: I,
    ) -> Result<(Vec<SubstateRecord>, HashSet<VersionedSubstateIdRef<'a>>), StorageError> {
        let mut substate_ids = requirements.into_iter().collect::<HashSet<_>>();
        let found = tx.substates_get_any(&substate_ids)?;
        for f in &found {
            substate_ids.remove(f.substate_id());
        }

        Ok((found, substate_ids))
    }

    pub fn get_all<'a, TTx: StateStoreReadTransaction, I: IntoIterator<Item = VersionedSubstateIdRef<'a>>>(
        tx: &TTx,
        requirements: I,
    ) -> Result<Vec<SubstateRecord>, StorageError> {
        let (found, missing) = Self::get_any(tx, requirements)?;
        if !missing.is_empty() {
            return Err(StorageError::NotFound {
                item: "SubstateRecord",
                key: missing.display().to_string(),
            });
        }
        Ok(found)
    }

    pub fn get_any_max_version<'a, TTx: StateStoreReadTransaction, I: IntoIterator<Item = &'a SubstateId>>(
        tx: &TTx,
        substate_ids: I,
    ) -> Result<(Vec<SubstateRecord>, HashSet<&'a SubstateId>), StorageError> {
        let mut substate_ids = substate_ids.into_iter().collect::<HashSet<_>>();
        let found = tx.substates_get_any_max_version(substate_ids.iter().copied())?;
        for f in &found {
            substate_ids.remove(&f.substate_id);
        }

        Ok((found, substate_ids))
    }

    /// Returns (version, is_up)
    pub fn get_latest_version<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        substate_id: &SubstateId,
    ) -> Result<(u32, bool), StorageError> {
        tx.substates_get_max_version_for_substate(substate_id)
    }

    pub fn get_latest<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        substate_id: &SubstateId,
    ) -> Result<SubstateRecord, StorageError> {
        let (max_version, _) = Self::get_latest_version(tx, substate_id)?;
        let rec = Self::get(tx, &SubstateAddress::from_substate_id(substate_id, max_version))?;
        Ok(rec)
    }

    pub fn get_created_quorum_certificate<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
    ) -> Result<QuorumCertificate, StorageError> {
        tx.quorum_certificates_get(self.created_justify())
    }

    pub fn get_destroyed_quorum_certificate<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
    ) -> Result<Option<QuorumCertificate>, StorageError> {
        self.destroyed()
            .map(|destroyed| tx.quorum_certificates_get(&destroyed.justify))
            .transpose()
    }

    pub fn destroy<TTx: StateStoreWriteTransaction>(
        tx: &mut TTx,
        versioned_substate_id: VersionedSubstateId,
        shard: Shard,
        epoch: Epoch,
        destroyed_by_block: NodeHeight,
        destroyed_justify: &QcId,
    ) -> Result<(), StorageError> {
        tx.substates_down(
            versioned_substate_id,
            shard,
            epoch,
            destroyed_by_block,
            destroyed_justify,
        )
    }

    pub fn prune_downed_values<TTx: StateStoreWriteTransaction>(
        tx: &mut TTx,
        epoch: Epoch,
    ) -> Result<(), StorageError> {
        tx.substates_prune_downed_values(epoch)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubstateCreatedProof {
    pub substate: SubstateData,
    // TODO: proof that data was created
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubstateDestroyedProof {
    pub substate_id: SubstateId,
    pub version: u32,
    // TODO: proof that data was destroyed
}

impl SubstateDestroyedProof {
    pub fn to_versioned_substate_id(&self) -> VersionedSubstateId {
        VersionedSubstateId::new(self.substate_id.clone(), self.version)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubstateValueOrHash {
    Value(SubstateValue),
    Hash(FixedHash),
}

impl SubstateValueOrHash {
    pub fn value(&self) -> Option<&SubstateValue> {
        match self {
            SubstateValueOrHash::Value(value) => Some(value),
            SubstateValueOrHash::Hash(_) => None,
        }
    }

    pub fn into_value(self) -> Option<SubstateValue> {
        match self {
            SubstateValueOrHash::Value(value) => Some(value),
            SubstateValueOrHash::Hash(_) => None,
        }
    }

    pub fn hash(&self) -> Option<&FixedHash> {
        match self {
            SubstateValueOrHash::Value(_) => None,
            SubstateValueOrHash::Hash(hash) => Some(hash),
        }
    }

    pub fn to_value_hash(&self, version: u32) -> FixedHash {
        match &self {
            SubstateValueOrHash::Value(v) => hash_substate(v, version),
            SubstateValueOrHash::Hash(hash) => *hash,
        }
    }
}

impl From<SubstateValue> for SubstateValueOrHash {
    fn from(value: SubstateValue) -> Self {
        Self::Value(value)
    }
}

impl From<FixedHash> for SubstateValueOrHash {
    fn from(hash: FixedHash) -> Self {
        Self::Hash(hash)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubstateData {
    pub substate_id: SubstateId,
    pub version: u32,
    pub value: SubstateValueOrHash,
}

impl SubstateData {
    pub fn value(&self) -> &SubstateValueOrHash {
        &self.value
    }

    pub fn as_versioned_substate_id_ref(&self) -> VersionedSubstateIdRef<'_> {
        VersionedSubstateIdRef::new(&self.substate_id, self.version)
    }

    pub fn to_value_hash(&self) -> FixedHash {
        self.value.to_value_hash(self.version)
    }

    pub fn substate_id(&self) -> &SubstateId {
        &self.substate_id
    }
}

impl From<SubstateRecord> for SubstateData {
    fn from(value: SubstateRecord) -> Self {
        Self {
            value: value
                .substate_value
                .map(Into::into)
                .unwrap_or_else(|| value.state_hash.into()),
            substate_id: value.substate_id,
            version: value.version,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubstateUpdate {
    Create(SubstateCreatedProof),
    Destroy(SubstateDestroyedProof),
}

impl SubstateUpdate {
    pub fn is_create(&self) -> bool {
        matches!(self, Self::Create(_))
    }

    pub fn is_destroy(&self) -> bool {
        matches!(self, Self::Destroy { .. })
    }

    pub fn substate_id(&self) -> &SubstateId {
        match self {
            Self::Create(create) => &create.substate.substate_id,
            Self::Destroy(destroyed) => &destroyed.substate_id,
        }
    }

    pub fn version(&self) -> u32 {
        match self {
            Self::Create(create) => create.substate.version,
            Self::Destroy(destroyed) => destroyed.version,
        }
    }

    pub fn to_versioned_substate_id(&self) -> VersionedSubstateId {
        VersionedSubstateId::new(self.substate_id().clone(), self.version())
    }

    pub fn as_create(&self) -> Option<&SubstateCreatedProof> {
        match self {
            Self::Create(create) => Some(create),
            _ => None,
        }
    }
}

impl From<SubstateCreatedProof> for SubstateUpdate {
    fn from(value: SubstateCreatedProof) -> Self {
        Self::Create(value)
    }
}

impl Display for SubstateUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create(proof) => write!(f, "Create: {}(v{})", proof.substate.substate_id, proof.substate.version),
            Self::Destroy(proof) => write!(f, "Destroy: {}(v{})", proof.substate_id, proof.version),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SubstateLockState {
    /// The lock was successfully acquired
    LockAcquired,
    /// The lock was not acquired because some substates are DOWN
    SomeDestroyed,
    /// Some substates are locked for write
    SomeAlreadyWriteLocked,
    /// Some outputs substates exist. This indicates that that we attempted to lock an output but the output is already
    /// a substate (Up or DOWN)
    SomeOutputSubstatesExist,
}

impl SubstateLockState {
    pub fn is_acquired(&self) -> bool {
        matches!(self, Self::LockAcquired)
    }
}
