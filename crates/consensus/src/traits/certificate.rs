//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use log::info;
use tari_consensus_types::{HighPc, HighTc, PcId, ProposalCertificate, TcId, TimeoutCertificate};
use tari_ootle_common_types::{Epoch, displayable::Displayable, optional::Optional};
use tari_ootle_storage::{
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
    consensus_models::{Block, BookkeepingModel},
};

const LOG_TARGET: &str = "tari::ootle::consensus::quorum_certificate";

pub trait CertificateStore: Sized {
    type Id: 'static;
    type HighCertificate;
    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch, id: &Self::Id) -> Result<Self, StorageError>;

    fn get_many<'a, TTx, I>(tx: &TTx, ids: I) -> Result<Vec<Self>, StorageError>
    where
        TTx: StateStoreReadTransaction,
        I: IntoIterator<Item = &'a (Epoch, Self::Id)>,
        I::IntoIter: ExactSizeIterator;

    fn save<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError>;

    fn update_highest<TTx>(&self, tx: &mut TTx) -> Result<Self::HighCertificate, StorageError>
    where
        TTx: StateStoreWriteTransaction + Deref,
        TTx::Target: StateStoreReadTransaction;
}

impl CertificateStore for ProposalCertificate {
    type HighCertificate = HighPc;
    type Id = PcId;

    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch, id: &Self::Id) -> Result<Self, StorageError> {
        tx.proposal_certificates_get(epoch, id)
    }

    fn get_many<'a, TTx, I>(tx: &TTx, ids: I) -> Result<Vec<Self>, StorageError>
    where
        TTx: StateStoreReadTransaction,
        I: IntoIterator<Item = &'a (Epoch, Self::Id)>,
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
            Some(high_pc) if high_pc.height() >= self.height() => {
                info!(
                    target: LOG_TARGET,
                    "🔥 HIGH_PC ({}, previous high PC: {} {}) - not new",
                    self,
                    high_pc.block_id(),
                    high_pc.height(),
                );
                Ok(high_pc)
            },
            maybe_high_pc => {
                // The certificate id and the block id omit the height, epoch and shard group, and the signed vote
                // message omits the shard group, so the stored block is the authority for all three. A certificate of
                // the zero block justifies the epoch's genesis block, which is the authority for its shard group.
                if self.justifies_zero_block() {
                    let genesis = Block::get_genesis_for_epoch(&**tx, self.epoch())?;
                    if genesis.shard_group() != self.shard_group() {
                        return Err(StorageError::DataInconsistency {
                            details: format!(
                                "Zero-block certificate for epoch {} claims shard group {}, but genesis block {} is \
                                 for shard group {}",
                                self.epoch(),
                                self.shard_group(),
                                genesis.id(),
                                genesis.shard_group(),
                            ),
                        });
                    }
                } else {
                    let block = Block::get(&**tx, &self.calculate_block_id())?;
                    if block.height() != self.height() ||
                        block.epoch() != self.epoch() ||
                        block.shard_group() != self.shard_group()
                    {
                        return Err(StorageError::DataInconsistency {
                            details: format!(
                                "Certificate {} claims height {} in epoch {} for shard group {}, but block {} is at \
                                 height {} in epoch {} for shard group {}",
                                self.calculate_id(),
                                self.height(),
                                self.epoch(),
                                self.shard_group(),
                                block.id(),
                                block.height(),
                                block.epoch(),
                                block.shard_group(),
                            ),
                        });
                    }
                }

                let high_pc = self.as_high_pc();
                info!(
                    target: LOG_TARGET,
                    "🔥 NEW HIGH_PC ({}, previous high PC: {})",
                    self,
                    maybe_high_pc.display(),
                );

                // EDGE CASE: If we receive a new high PC, clear the last sent new view. This is because we could have
                // sent many unsuccessful NEWVIEWs (likely we're offline) and the chain progressed
                // without us. But then if we need to send NEWVIEWs again, it will be aligned with the network view.
                tx.last_sent_new_view_clear()?;
                self.save(tx)?;
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

    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch, id: &Self::Id) -> Result<Self, StorageError> {
        tx.timeout_certificates_get(epoch, id)
    }

    fn get_many<'a, TTx, I>(tx: &TTx, ids: I) -> Result<Vec<Self>, StorageError>
    where
        TTx: StateStoreReadTransaction,
        I: IntoIterator<Item = &'a (Epoch, Self::Id)>,
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

#[cfg(test)]
mod tests {
    use tari_consensus_types::{LastSentNewView, ProposalCertificate, ShardGroupAccumulatedData};
    use tari_crypto::tari_utilities::epoch_time::EpochTime;
    use tari_ootle_common_types::{
        Epoch,
        ExtraData,
        NodeHeight,
        NumPreshards,
        ProtocolVersion,
        ShardGroup,
        optional::Optional,
    };
    use tari_ootle_storage::{
        StateStore,
        StorageError,
        consensus_models::{Block, BlockCommands, BlockHeader, BookkeepingModel},
    };
    use tari_ootle_transaction::Network;
    use tari_sidechain::{FixedHash, QuorumDecision};
    use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};
    use tari_template_lib_types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

    use super::CertificateStore;

    fn make_block_and_pc(parent: &Block, height: NodeHeight) -> (Block, ProposalCertificate) {
        let shard_group = ShardGroup::all_shards(NumPreshards::P256);
        let commands = BlockCommands::empty();
        let mut header = BlockHeader::create_unsigned(
            Network::LocalNet,
            ProtocolVersion::V0,
            *parent.id(),
            parent.justify().calculate_id(),
            None,
            height,
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
        header.set_signature(SchnorrSignatureBytes::zero());
        let block = Block::new(header, parent.justify().clone(), commands, None);
        let pc = ProposalCertificate::new(
            block.header().calculate_hash(),
            *block.parent(),
            block.height(),
            block.epoch(),
            shard_group,
            ProposalCertificate::genesis_signatures(),
            QuorumDecision::Accept,
        );
        (block, pc)
    }

    #[test]
    fn update_highest_clears_last_sent_new_view_only_on_new_high_pc() {
        let tmp = tempfile::tempdir().unwrap();
        let store: RocksDbStateStore<String> = RocksDbStateStore::open(tmp.path(), DatabaseOptions::default()).unwrap();

        let zero = Block::zero_block(Network::LocalNet, NumPreshards::P256);
        let (b1, pc1) = make_block_and_pc(&zero, NodeHeight(1));
        let (b2, pc2) = make_block_and_pc(&b1, NodeHeight(2));

        store
            .with_write_tx(|tx| {
                zero.insert(tx)?;
                b1.insert(tx)?;
                b2.insert(tx)?;

                pc1.update_highest(tx)?;
                LastSentNewView {
                    epoch: Epoch::zero(),
                    height: NodeHeight(10),
                }
                .set(tx)?;

                // Re-observing a non-new PC (height <= HighPc height) must preserve LastSentNewView.
                pc1.update_highest(tx)?;
                let preserved = LastSentNewView::get(&**tx, Epoch::zero())
                    .optional()?
                    .map(|v| (v.epoch, v.height));
                assert_eq!(preserved, Some((Epoch::zero(), NodeHeight(10))));

                // Receiving a strictly higher PC must clear LastSentNewView.
                let new_high = pc2.update_highest(tx)?;
                assert_eq!(new_high.height(), NodeHeight(2));
                assert!(LastSentNewView::get(&**tx, Epoch::zero()).optional()?.is_none());
                Ok::<_, StorageError>(())
            })
            .unwrap();
    }
}
