//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use log::info;
use tari_consensus_types::{HighPc, HighTc, ProposalCertificate, QcId, TcId, TimeoutCertificate};
use tari_dan_common_types::{displayable::Displayable, optional::Optional};
use tari_dan_storage::{
    consensus_models::BookkeepingModel,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};

const LOG_TARGET: &str = "tari::dan::consensus::quorum_certificate";

pub trait CertificateStore: Sized {
    type Id: 'static;
    type HighCertificate;
    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, id: &Self::Id) -> Result<Self, StorageError>;

    fn get_many<'a, TTx, I>(tx: &TTx, ids: I) -> Result<Vec<Self>, StorageError>
    where
        TTx: StateStoreReadTransaction,
        I: IntoIterator<Item = &'a Self::Id>,
        I::IntoIter: ExactSizeIterator;

    fn save<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError>;

    fn update_highest<TTx>(&self, tx: &mut TTx) -> Result<Self::HighCertificate, StorageError>
    where
        TTx: StateStoreWriteTransaction + Deref,
        TTx::Target: StateStoreReadTransaction;
}

impl CertificateStore for ProposalCertificate {
    type HighCertificate = HighPc;
    type Id = QcId;

    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, id: &Self::Id) -> Result<Self, StorageError> {
        tx.proposal_certificates_get(id)
    }

    fn get_many<'a, TTx, I>(tx: &TTx, ids: I) -> Result<Vec<Self>, StorageError>
    where
        TTx: StateStoreReadTransaction,
        I: IntoIterator<Item = &'a Self::Id>,
        I::IntoIter: ExactSizeIterator,
    {
        tx.proposal_certificates_get_many(ids)
    }

    fn save<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.proposal_certificates_save(self)
    }

    fn update_highest<TTx>(&self, tx: &mut TTx) -> Result<Self::HighCertificate, StorageError>
    where
        TTx: StateStoreWriteTransaction + Deref,
        TTx::Target: StateStoreReadTransaction,
    {
        match HighPc::get(&**tx, self.epoch()).optional()? {
            Some(high_pc) if high_pc.block_height() >= self.height() => {
                info!(
                    target: LOG_TARGET,
                    "🔥 HIGH_PC ({}, previous high PC: {} {}) - not new",
                    self,
                    high_pc.block_id(),
                    high_pc.block_height(),
                );
                Ok(high_pc)
            },
            Some(_) | None => {
                let high_pc = self.as_high_pc();
                info!(
                    target: LOG_TARGET,
                    "🔥 NEW HIGH_PC ({}, previous high PC: {} {})",
                    self,
                    high_pc.block_id(),
                    high_pc.block_height(),
                );

                self.save(tx)?;
                // This will fail if the block doesnt exist
                self.as_leaf_block().set(tx)?;
                high_pc.set(tx)?;
                Ok(high_pc)
            },
        }
    }
}

impl CertificateStore for TimeoutCertificate {
    type HighCertificate = HighTc;
    type Id = TcId;

    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, id: &Self::Id) -> Result<Self, StorageError> {
        tx.timeout_certificates_get(id)
    }

    fn get_many<'a, TTx, I>(tx: &TTx, ids: I) -> Result<Vec<Self>, StorageError>
    where
        TTx: StateStoreReadTransaction,
        I: IntoIterator<Item = &'a Self::Id>,
        I::IntoIter: ExactSizeIterator,
    {
        tx.timeout_certificates_get_many(ids)
    }

    fn save<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.timeout_certificates_save(self)
    }

    fn update_highest<TTx>(&self, tx: &mut TTx) -> Result<Self::HighCertificate, StorageError>
    where
        TTx: StateStoreWriteTransaction + Deref,
        TTx::Target: StateStoreReadTransaction,
    {
        match HighTc::get(&**tx, self.epoch()).optional()? {
            Some(high_tc) if high_tc.height() >= self.height() => {
                info!(
                    target: LOG_TARGET,
                    "🕒️ HIGH_TC ({}, previous high TC: {} {}) - not new",
                    self,
                    high_tc.id(),
                    high_tc.height(),
                );
                Ok(high_tc)
            },
            maybe_high_tc => {
                let high_tc = self.as_high_tc();
                info!(
                    target: LOG_TARGET,
                    "🕒️ NEW HIGH_TC ({}, previous high TC: {})",
                    self,
                    maybe_high_tc.display(),
                );
                self.save(tx)?;
                high_tc.set(tx)?;
                Ok(high_tc)
            },
        }
    }
}
