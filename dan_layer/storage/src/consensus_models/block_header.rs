//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Debug, Display, Formatter},
};

use serde::{Deserialize, Serialize};
use tari_common::configuration::Network;
use tari_common_types::types::{FixedHash, PublicKey};
use tari_crypto::tari_utilities::epoch_time::EpochTime;
use tari_dan_common_types::{hashing, shard::Shard, Epoch, ExtraData, NodeHeight, NumPreshards, ShardGroup};
use tari_state_tree::{compute_merkle_root_for_hashes, TreeHash};
#[cfg(feature = "ts")]
use ts_rs::TS;

use super::{BlockError, BlockId, QcId, QuorumCertificate, ValidatorSchnorrSignature};
use crate::consensus_models::{Command, LastExecuted, LastProposed, LastVoted, LeafBlock, LockedBlock};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, export_to = "../../bindings/src/types/"))]
pub struct BlockHeader {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    id: BlockId,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    network: Network,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    parent: BlockId,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    justify_id: QcId,
    height: NodeHeight,
    epoch: Epoch,
    shard_group: ShardGroup,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    proposed_by: PublicKey,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    total_leader_fee: u64,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    state_merkle_root: FixedHash,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    command_merkle_root: FixedHash,
    /// If the block is a dummy block.
    is_dummy: bool,
    /// Counter for each foreign shard for reliable broadcast.
    foreign_indexes: BTreeMap<Shard, u64>,
    /// Signature of block by the proposer.
    #[cfg_attr(feature = "ts", ts(type = "{public_nonce : string, signature: string} | null"))]
    signature: Option<ValidatorSchnorrSignature>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    timestamp: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    base_layer_block_height: u64,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    base_layer_block_hash: FixedHash,
    extra_data: ExtraData,
}

impl BlockHeader {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        network: Network,
        parent: BlockId,
        justify_id: QcId,
        height: NodeHeight,
        epoch: Epoch,
        shard_group: ShardGroup,
        proposed_by: PublicKey,
        state_merkle_root: FixedHash,
        commands: &BTreeSet<Command>,
        total_leader_fee: u64,
        sorted_foreign_indexes: BTreeMap<Shard, u64>,
        signature: Option<ValidatorSchnorrSignature>,
        timestamp: u64,
        base_layer_block_height: u64,
        base_layer_block_hash: FixedHash,
        extra_data: ExtraData,
    ) -> Result<Self, BlockError> {
        let command_merkle_root = Self::compute_command_merkle_root(commands)?;
        let mut header = BlockHeader {
            id: BlockId::zero(),
            network,
            parent,
            justify_id,
            height,
            epoch,
            shard_group,
            proposed_by,
            state_merkle_root,
            command_merkle_root,
            total_leader_fee,
            is_dummy: false,
            foreign_indexes: sorted_foreign_indexes,
            signature,
            timestamp,
            base_layer_block_height,
            base_layer_block_hash,
            extra_data,
        };
        header.id = header.calculate_id();

        Ok(header)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn load(
        id: BlockId,
        network: Network,
        parent: BlockId,
        justify_id: QcId,
        height: NodeHeight,
        epoch: Epoch,
        shard_group: ShardGroup,
        proposed_by: PublicKey,
        state_merkle_root: FixedHash,
        total_leader_fee: u64,
        is_dummy: bool,
        sorted_foreign_indexes: BTreeMap<Shard, u64>,
        signature: Option<ValidatorSchnorrSignature>,
        timestamp: u64,
        base_layer_block_height: u64,
        base_layer_block_hash: FixedHash,
        extra_data: ExtraData,
        command_merkle_root: FixedHash,
    ) -> Self {
        Self {
            id,
            network,
            parent,
            justify_id,
            height,
            epoch,
            shard_group,
            proposed_by,
            state_merkle_root,
            command_merkle_root,
            total_leader_fee,
            is_dummy,
            foreign_indexes: sorted_foreign_indexes,
            signature,
            timestamp,
            base_layer_block_height,
            base_layer_block_hash,
            extra_data,
        }
    }

    /// This is the parent block for all genesis blocks. Its block ID is always zero.
    pub fn zero_block(network: Network, num_preshards: NumPreshards) -> Self {
        Self {
            network,
            id: BlockId::zero(),
            parent: BlockId::zero(),
            justify_id: *QuorumCertificate::genesis(Epoch::zero(), ShardGroup::all_shards(num_preshards)).id(),
            height: NodeHeight::zero(),
            epoch: Epoch::zero(),
            shard_group: ShardGroup::all_shards(num_preshards),
            proposed_by: PublicKey::default(),
            state_merkle_root: FixedHash::zero(),
            command_merkle_root: FixedHash::zero(),
            total_leader_fee: 0,
            is_dummy: false,
            foreign_indexes: BTreeMap::new(),
            signature: None,
            timestamp: EpochTime::now().as_u64(),
            base_layer_block_height: 0,
            base_layer_block_hash: FixedHash::zero(),
            extra_data: ExtraData::new(),
        }
    }

    pub fn dummy_block(
        network: Network,
        parent: BlockId,
        proposed_by: PublicKey,
        height: NodeHeight,
        justify_id: QcId,
        epoch: Epoch,
        shard_group: ShardGroup,
        parent_state_merkle_root: FixedHash,
        parent_timestamp: u64,
        parent_base_layer_block_height: u64,
        parent_base_layer_block_hash: FixedHash,
    ) -> Self {
        let mut block = Self {
            id: BlockId::zero(),
            network,
            parent,
            justify_id,
            height,
            epoch,
            shard_group,
            proposed_by,
            state_merkle_root: parent_state_merkle_root,
            command_merkle_root: BlockHeader::compute_command_merkle_root(&BTreeSet::new())
                .expect("compute_command_merkle_root is infallible for empty commands"),
            total_leader_fee: 0,
            is_dummy: true,
            foreign_indexes: BTreeMap::new(),
            signature: None,
            timestamp: parent_timestamp,
            base_layer_block_height: parent_base_layer_block_height,
            base_layer_block_hash: parent_base_layer_block_hash,
            extra_data: ExtraData::new(),
        };
        block.id = block.calculate_id();
        block
    }

    pub fn calculate_id(&self) -> BlockId {
        // Hash is created from the hash of the "body" and
        // then hashed with the parent, so that you can
        // create a merkle proof of a chain of blocks
        // ```pre
        // root
        // |\
        // |  block1
        // |\
        // |  block2
        // |
        // blockbody
        // ```

        let header_hash = self.calculate_hash();
        Self::calculate_block_id(&header_hash, &self.parent)
    }

    pub(crate) fn calculate_block_id(contents_hash: &FixedHash, parent_id: &BlockId) -> BlockId {
        if *contents_hash == FixedHash::zero() && parent_id.is_zero() {
            return BlockId::zero();
        }

        hashing::block_hasher()
            .chain(parent_id)
            .chain(contents_hash)
            .finalize_into_array()
            .into()
    }

    pub fn create_extra_data_hash(&self) -> FixedHash {
        hashing::extra_data_hasher().chain(&self.extra_data).finalize().into()
    }

    pub fn create_foreign_indexes_hash(&self) -> FixedHash {
        hashing::foreign_indexes_hasher()
            .chain(&self.foreign_indexes)
            .finalize()
            .into()
    }

    pub fn calculate_hash(&self) -> FixedHash {
        // These hashes reduce proof sizes, specifically, a proof-of-commit only needs to include these hashes and not
        // their data.
        let extra_data_hash = self.create_extra_data_hash();
        let foreign_indexes_hash = self.create_foreign_indexes_hash();

        hashing::block_hasher()
            .chain(&self.network.as_byte())
            .chain(&self.justify_id)
            .chain(&self.height)
            .chain(&self.total_leader_fee)
            .chain(&self.epoch)
            .chain(&self.shard_group)
            .chain(&self.proposed_by)
            .chain(&self.state_merkle_root)
            .chain(&self.is_dummy)
            .chain(&self.command_merkle_root)
            .chain(&foreign_indexes_hash)
            .chain(&self.timestamp)
            .chain(&self.base_layer_block_height)
            .chain(&self.base_layer_block_hash)
            .chain(&extra_data_hash)
            .finalize()
            .into()
    }

    pub fn is_genesis(&self) -> bool {
        self.height.is_zero()
    }

    pub fn as_locked_block(&self) -> LockedBlock {
        LockedBlock {
            height: self.height,
            block_id: self.id,
            epoch: self.epoch,
        }
    }

    pub fn as_last_executed(&self) -> LastExecuted {
        LastExecuted {
            height: self.height,
            block_id: self.id,
            epoch: self.epoch,
        }
    }

    pub fn as_last_voted(&self) -> LastVoted {
        LastVoted {
            height: self.height,
            block_id: self.id,
            epoch: self.epoch,
        }
    }

    pub fn as_leaf_block(&self) -> LeafBlock {
        LeafBlock {
            height: self.height,
            block_id: self.id,
            epoch: self.epoch,
        }
    }

    pub fn as_last_proposed(&self) -> LastProposed {
        LastProposed {
            height: self.height,
            block_id: self.id,
            epoch: self.epoch,
        }
    }

    pub fn id(&self) -> &BlockId {
        &self.id
    }

    pub fn network(&self) -> Network {
        self.network
    }

    pub fn parent(&self) -> &BlockId {
        &self.parent
    }

    pub fn justify_id(&self) -> &QcId {
        &self.justify_id
    }

    pub fn height(&self) -> NodeHeight {
        self.height
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    pub fn shard_group(&self) -> ShardGroup {
        self.shard_group
    }

    pub fn total_leader_fee(&self) -> u64 {
        self.total_leader_fee
    }

    pub fn total_transaction_fee(&self) -> u64 {
        self.total_leader_fee
    }

    pub fn proposed_by(&self) -> &PublicKey {
        &self.proposed_by
    }

    pub fn state_merkle_root(&self) -> &FixedHash {
        &self.state_merkle_root
    }

    pub fn command_merkle_root(&self) -> &FixedHash {
        &self.command_merkle_root
    }

    pub fn is_dummy(&self) -> bool {
        self.is_dummy
    }

    pub fn get_foreign_counter(&self, bucket: &Shard) -> Option<u64> {
        self.foreign_indexes.get(bucket).copied()
    }

    pub fn foreign_indexes(&self) -> &BTreeMap<Shard, u64> {
        &self.foreign_indexes
    }

    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    pub fn signature(&self) -> Option<&ValidatorSchnorrSignature> {
        self.signature.as_ref()
    }

    pub fn set_signature(&mut self, signature: ValidatorSchnorrSignature) {
        self.signature = Some(signature);
    }

    pub fn base_layer_block_height(&self) -> u64 {
        self.base_layer_block_height
    }

    pub fn base_layer_block_hash(&self) -> &FixedHash {
        &self.base_layer_block_hash
    }

    pub fn extra_data(&self) -> &ExtraData {
        &self.extra_data
    }

    pub fn compute_command_merkle_root(commands: &BTreeSet<Command>) -> Result<FixedHash, BlockError> {
        let hashes = commands.iter().map(|cmd| TreeHash::from(cmd.hash().into_array()));
        let hash = compute_merkle_root_for_hashes(hashes).map_err(BlockError::StateTreeError)?;
        Ok(FixedHash::from(hash.into_array()))
    }
}

impl Display for BlockHeader {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.is_dummy() {
            write!(f, "Dummy")?;
        }
        write!(
            f,
            "[{}, {}, {}, {}->{}]",
            self.height(),
            self.epoch(),
            self.shard_group(),
            self.id(),
            self.parent()
        )
    }
}
