//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::BTreeSet,
    fmt::{Debug, Display, Formatter},
};

use borsh::BorshSerialize;
use serde::{Deserialize, Serialize};
use tari_common_types::types::FixedHash;
use tari_consensus_types::{
    BlockId,
    LastExecuted,
    LastVoted,
    LeafBlock,
    LockedBlock,
    ProposalCertificate,
    QcId,
    SignedMessage,
    ToSignatureMessage,
};
use tari_crypto::tari_utilities::epoch_time::EpochTime;
use tari_engine_types::serde_with;
use tari_ootle_common_types::{hashing, Epoch, ExtraData, Network, NodeHeight, NumPreshards, ShardGroup};
use tari_sidechain::{BlockHeaderHashFields, BlockHeaderHashFieldsV1};
use tari_state_tree::{compute_merkle_root_for_hashes, TreeHash};
use tari_template_lib::{prelude::SchnorrSignatureBytes, types::crypto::RistrettoPublicKeyBytes};

use super::BlockError;
use crate::consensus_models::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct BlockHeader {
    /// "Cached" block ID/hash. This can be computed from the contents of the block header,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    id: BlockId,
    /// Network this block belongs to.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    network: Network,
    /// Parent block ID.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    parent: BlockId,
    /// The quorum certificate proposed in this block. Note that this QC justifies a previous block.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    justify_id: QcId,
    /// Block height.
    height: NodeHeight,
    /// Epoch this block belongs to.
    epoch: Epoch,
    /// Shard group that created this block.
    shard_group: ShardGroup,
    /// The public key of the proposer.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    proposed_by: RistrettoPublicKeyBytes,
    /// The total leader fee for this block. This should match the sum of the leader fees in the block's body.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    total_leader_fee: u64,
    /// A Merkle root hash committing to all state after this block has been applied.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[serde(with = "serde_with::hex")]
    state_merkle_root: FixedHash,
    /// A Merkle root hash committing to commands in this block. It is zero if the block has no commands.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[serde(with = "serde_with::hex")]
    command_merkle_root: FixedHash,
    /// Proposer signature that signs the Block ID
    signature: Option<SchnorrSignatureBytes>,
    /// The time indicating the creation time of the block. Currently, this can be chosen arbitrarily and is only
    /// informational/used for metrics.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    timestamp: u64,
    /// The epoch hash is a hash given by the epoch oracle. E.g. the base layer epoch oracle gives the first block hash
    /// of the epoch.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[serde(with = "serde_with::hex")]
    epoch_hash: FixedHash,
    /// Extra data to allow for potential future data to be provided as necessary without breaking changes.
    /// Currently, this is used to store the block's sidechain_id (if applicable).
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
        proposed_by: RistrettoPublicKeyBytes,
        state_merkle_root: FixedHash,
        commands: &BTreeSet<Command>,
        total_leader_fee: u64,
        signature: SchnorrSignatureBytes,
        timestamp: u64,
        epoch_hash: FixedHash,
        extra_data: ExtraData,
    ) -> Result<Self, BlockError> {
        let mut header = Self::create_unsigned(
            network,
            parent,
            justify_id,
            height,
            epoch,
            shard_group,
            proposed_by,
            state_merkle_root,
            commands,
            total_leader_fee,
            timestamp,
            epoch_hash,
            extra_data,
        )?;

        header.set_signature(signature);

        Ok(header)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_unsigned(
        network: Network,
        parent: BlockId,
        justify_id: QcId,
        height: NodeHeight,
        epoch: Epoch,
        shard_group: ShardGroup,
        proposed_by: RistrettoPublicKeyBytes,
        state_merkle_root: FixedHash,
        commands: &BTreeSet<Command>,
        total_leader_fee: u64,
        timestamp: u64,
        epoch_hash: FixedHash,
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
            signature: None,
            timestamp,
            epoch_hash,
            extra_data,
        };
        header.id = header.calculate_id();

        Ok(header)
    }

    pub fn genesis(
        network: Network,
        justify_id: QcId,
        epoch: Epoch,
        shard_group: ShardGroup,
        state_merkle_root: FixedHash,
        epoch_hash: FixedHash,
        extra_data: ExtraData,
    ) -> Self {
        Self::create(
            network,
            BlockId::zero(),
            justify_id,
            NodeHeight::zero(),
            epoch,
            shard_group,
            RistrettoPublicKeyBytes::default(),
            state_merkle_root,
            &BTreeSet::new(),
            0,
            SchnorrSignatureBytes::zero(),
            0,
            epoch_hash,
            extra_data,
        )
        .expect("Infallible with empty commands")
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
        proposed_by: RistrettoPublicKeyBytes,
        state_merkle_root: FixedHash,
        total_leader_fee: u64,
        signature: Option<SchnorrSignatureBytes>,
        timestamp: u64,
        epoch_hash: FixedHash,
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
            signature,
            timestamp,
            epoch_hash,
            extra_data,
        }
    }

    /// This is the parent block for all genesis blocks. Its block ID is always zero.
    pub fn zero_block(network: Network, num_preshards: NumPreshards) -> Self {
        Self {
            network,
            id: BlockId::zero(),
            parent: BlockId::zero(),
            justify_id: ProposalCertificate::genesis(Epoch::zero(), ShardGroup::all_shards(num_preshards))
                .calculate_id(),
            height: NodeHeight::zero(),
            epoch: Epoch::zero(),
            shard_group: ShardGroup::all_shards(num_preshards),
            proposed_by: RistrettoPublicKeyBytes::default(),
            state_merkle_root: FixedHash::zero(),
            command_merkle_root: FixedHash::zero(),
            total_leader_fee: 0,
            // Not a dummy block
            signature: Some(SchnorrSignatureBytes::zero()),
            timestamp: EpochTime::now().as_u64(),
            epoch_hash: FixedHash::zero(),
            extra_data: ExtraData::new(),
        }
    }

    pub fn dummy_block(
        network: Network,
        parent: BlockId,
        proposed_by: RistrettoPublicKeyBytes,
        height: NodeHeight,
        justify_id: QcId,
        epoch: Epoch,
        shard_group: ShardGroup,
        parent_state_merkle_root: FixedHash,
        parent_timestamp: u64,
        parent_epoch_hash: FixedHash,
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
            signature: None,
            timestamp: parent_timestamp,
            epoch_hash: parent_epoch_hash,
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
        Self::calculate_block_id(&self.parent, &header_hash)
    }

    pub(crate) fn calculate_block_id(parent_id: &BlockId, header_hash: &FixedHash) -> BlockId {
        // The zero block is a special case. It has no parent and its ID is always zero.
        if *header_hash == FixedHash::zero() && parent_id.is_zero() {
            return BlockId::zero();
        }

        hashing::block_hasher()
            .chain(parent_id)
            .chain(header_hash)
            .finalize_into_array()
            .into()
    }

    pub fn calculate_metadata_hash(&self) -> FixedHash {
        let fields = MetadataHashFields::V1(MetadataHashFieldsV1 {
            total_leader_fee: self.total_leader_fee,
            timestamp: self.timestamp,
            epoch_hash: &self.epoch_hash,
            extra_data: &self.extra_data,
        });
        hashing::block_metadata_hasher().chain(&fields).finalize().into()
    }

    pub fn calculate_hash(&self) -> FixedHash {
        // This hash reduces proof sizes. A proof-of-commit only needs to include this hash and not
        // the data.
        let metadata_hash = self.calculate_metadata_hash();

        let fields = BlockHeaderHashFields::V1(BlockHeaderHashFieldsV1 {
            network: self.network.as_byte(),
            justify_id: self.justify_id.hash(),
            height: self.height.as_u64(),
            epoch: self.epoch.as_u64(),
            shard_group: tari_sidechain::ShardGroup {
                start: self.shard_group.start().as_u32(),
                end_inclusive: self.shard_group.end().as_u32(),
            },
            proposed_by: self.proposed_by.as_bytes(),
            state_merkle_root: &self.state_merkle_root,
            command_merkle_root: &self.command_merkle_root,
            metadata_hash: &metadata_hash,
        });

        hashing::block_hasher().chain(&fields).finalize().into()
    }

    pub fn is_genesis(&self) -> bool {
        // TODO: simplify genesis - This check is used to skip some validations (e.g. signature). Are there some
        // malicious tricks with the other fields here? Ideally we'd simple do
        // `self == Self::genesis(self.epoch, self.shard_group)` however the previous epoch state hash makes that
        // difficult.
        self.height.is_zero() &&
            self.parent.is_zero() &&
            self.timestamp == 0 &&
            self.command_merkle_root.iter().all(|b| *b == 0) &&
            self.proposed_by.iter().all(|b| *b == 0) &&
            self.signature.is_none()
    }

    pub fn as_locked(&self) -> LockedBlock {
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

    pub fn as_leaf(&self) -> LeafBlock {
        LeafBlock {
            height: self.height,
            block_id: self.id,
            epoch: self.epoch,
            shard_group: self.shard_group,
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

    pub fn proposed_by(&self) -> &RistrettoPublicKeyBytes {
        &self.proposed_by
    }

    pub fn state_merkle_root(&self) -> &FixedHash {
        &self.state_merkle_root
    }

    pub fn command_merkle_root(&self) -> &FixedHash {
        &self.command_merkle_root
    }

    pub fn is_dummy(&self) -> bool {
        self.signature.is_none()
    }

    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    pub fn signature(&self) -> Option<&SchnorrSignatureBytes> {
        self.signature.as_ref()
    }

    pub fn set_signature(&mut self, signature: SchnorrSignatureBytes) {
        self.signature = Some(signature);
    }

    pub fn epoch_hash(&self) -> &FixedHash {
        &self.epoch_hash
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

// Used to sign the block
impl ToSignatureMessage for BlockHeader {
    fn to_signature_message(&self) -> FixedHash {
        *self.id.hash()
    }
}

impl SignedMessage for BlockHeader {
    fn signature(&self) -> &SchnorrSignatureBytes {
        // TODO: remove the Option for signature
        self.signature.as_ref().expect("BlockHeader not signed")
    }

    fn public_key(&self) -> &RistrettoPublicKeyBytes {
        &self.proposed_by
    }
}

#[derive(Debug, BorshSerialize)]
enum MetadataHashFields<'a> {
    V1(MetadataHashFieldsV1<'a>),
}

#[derive(Debug, BorshSerialize)]
struct MetadataHashFieldsV1<'a> {
    total_leader_fee: u64,
    timestamp: u64,
    epoch_hash: &'a FixedHash,
    extra_data: &'a ExtraData,
}
