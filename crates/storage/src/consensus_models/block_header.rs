//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::BTreeSet,
    fmt::{Debug, Display, Formatter},
};

use borsh::BorshSerialize;
use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_common_types::types::FixedHash;
use tari_consensus_types::{
    BlockId,
    LastExecuted,
    LastVoted,
    LeafBlock,
    LockedBlock,
    PcId,
    ProposalCertificate,
    ShardGroupAccumulatedData,
    SignedMessage,
    TcId,
    ToSignatureMessage,
};
use tari_crypto::tari_utilities::epoch_time::EpochTime;
use tari_ootle_common_types::{Epoch, ExtraData, NodeHeight, NumPreshards, ProtocolVersion, ShardGroup, hashing};
use tari_ootle_transaction::Network;
use tari_sidechain::{BlockHeaderHashFields, BlockHeaderHashFieldsV1, BlockHeaderHashFieldsV2};
use tari_state_tree::{TreeHash, compute_merkle_root_for_hashes};
use tari_template_lib_types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

use super::{BlockError, Command};

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BlockHeader {
    /// "Cached" block ID/hash. This is computed from the contents of the block header.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[n(0)]
    id: BlockId,
    /// Network this block belongs to.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[n(1)]
    network: Network,
    /// Parent block ID.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[n(2)]
    parent: BlockId,
    /// The quorum certificate proposed in this block. Note that this QC justifies a previous block.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[n(3)]
    justify_id: PcId,
    /// Block height.
    #[n(4)]
    height: NodeHeight,
    /// Epoch this block belongs to.
    #[n(5)]
    epoch: Epoch,
    /// Shard group that created this block.
    #[n(6)]
    shard_group: ShardGroup,
    /// The public key of the proposer.
    #[n(7)]
    proposed_by: RistrettoPublicKeyBytes,
    /// The total leader fee for this block. This should match the sum of the leader fees in the block's body.
    #[n(8)]
    total_leader_fee: u64,
    /// A Merkle root hash committing to all state after this block has been applied.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[serde(with = "ootle_serde::hex")]
    #[n(9)]
    #[cbor(with = "tari_bor::adapters::serde_bridge")]
    state_merkle_root: FixedHash,
    /// A Merkle root hash committing to commands in this block. It is zero if the block has no commands.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[serde(with = "ootle_serde::hex")]
    #[n(10)]
    #[cbor(with = "tari_bor::adapters::serde_bridge")]
    command_merkle_root: FixedHash,
    /// Proposer signature that signs the Block ID
    #[n(11)]
    signature: Option<SchnorrSignatureBytes>,
    /// The Unix Epoch timestamp indicating the creation time of the block. Currently, this can be chosen arbitrarily
    /// and is only informational/used for metrics.
    #[n(12)]
    timestamp: u64,
    /// The epoch hash is a hash given by the epoch oracle. E.g. the base layer epoch oracle gives the first block hash
    /// of the epoch.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[serde(with = "ootle_serde::hex")]
    #[n(13)]
    #[cbor(with = "tari_bor::adapters::serde_bridge")]
    epoch_hash: FixedHash,
    /// Accumulated data for the shard group up to and including this block.
    #[n(14)]
    accumulated_data: ShardGroupAccumulatedData,
    /// Extra data to allow for potential future data to be provided as necessary without breaking changes.
    /// Currently, this is used to store the block's sidechain_id (if applicable).
    #[n(15)]
    extra_data: ExtraData,
    /// The protocol version this block was produced under, resolved from the network's activation schedule at
    /// [`Self::epoch`]. It makes the block self-describing: [`Self::calculate_hash`] and the L1 verifier in
    /// `tari_sidechain` both select the hash schema from this field rather than from the schedule.
    ///
    /// A header that carries no version is under [`ProtocolVersion::V0`], so blocks written before the field
    /// existed decode and hash unchanged.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    #[serde(default)]
    #[n(16)]
    #[cbor(default)]
    protocol_version: ProtocolVersion,
    /// The id of the timeout certificate this block carries, or `None` when it carries none. From
    /// [`ProtocolVersion::V1`] it is part of the metadata hash and therefore of the signed block id, so a validity
    /// rule that reads the certificate (`check_justify_reaches_timeout_certificate`) reads data the proposer signed.
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    #[n(17)]
    timeout_certificate_id: Option<TcId>,
}

impl BlockHeader {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        network: Network,
        protocol_version: ProtocolVersion,
        parent: BlockId,
        justify_id: PcId,
        timeout_certificate_id: Option<TcId>,
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
        accumulated_data: ShardGroupAccumulatedData,
        extra_data: ExtraData,
    ) -> Result<Self, BlockError> {
        let mut header = Self::create_unsigned(
            network,
            protocol_version,
            parent,
            justify_id,
            timeout_certificate_id,
            height,
            epoch,
            shard_group,
            proposed_by,
            state_merkle_root,
            commands,
            total_leader_fee,
            timestamp,
            epoch_hash,
            accumulated_data,
            extra_data,
        )?;

        header.set_signature(signature);

        Ok(header)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_unsigned(
        network: Network,
        protocol_version: ProtocolVersion,
        parent: BlockId,
        justify_id: PcId,
        timeout_certificate_id: Option<TcId>,
        height: NodeHeight,
        epoch: Epoch,
        shard_group: ShardGroup,
        proposed_by: RistrettoPublicKeyBytes,
        state_merkle_root: FixedHash,
        commands: &BTreeSet<Command>,
        total_leader_fee: u64,
        timestamp: u64,
        epoch_hash: FixedHash,
        accumulated_data: ShardGroupAccumulatedData,
        extra_data: ExtraData,
    ) -> Result<Self, BlockError> {
        let command_merkle_root = Self::compute_command_merkle_root(commands)?;
        let mut header = BlockHeader {
            id: BlockId::zero(),
            network,
            protocol_version,
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
            accumulated_data,
            extra_data,
            timeout_certificate_id,
        };
        header.id = header.calculate_id();

        Ok(header)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn genesis(
        network: Network,
        protocol_version: ProtocolVersion,
        justify_id: PcId,
        epoch: Epoch,
        shard_group: ShardGroup,
        state_merkle_root: FixedHash,
        epoch_hash: FixedHash,
        accumulated_data: ShardGroupAccumulatedData,
        extra_data: ExtraData,
    ) -> Self {
        Self::create(
            network,
            protocol_version,
            BlockId::zero(),
            justify_id,
            None,
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
            accumulated_data,
            extra_data,
        )
        .expect("Infallible with empty commands")
    }

    /// This is the parent block for all genesis blocks. Its block ID is always zero.
    // TODO: do we need a zero block anymore?
    pub fn zero_block(network: Network, num_preshards: NumPreshards) -> Self {
        let shard_group = ShardGroup::all_shards(num_preshards);
        Self {
            network,
            protocol_version: ProtocolVersion::at(network, Epoch::zero()),
            id: BlockId::zero(),
            parent: BlockId::zero(),
            justify_id: ProposalCertificate::genesis(Epoch::zero(), ShardGroup::all_shards(num_preshards))
                .calculate_id(),
            height: NodeHeight::zero(),
            epoch: Epoch::zero(),
            shard_group,
            proposed_by: RistrettoPublicKeyBytes::default(),
            state_merkle_root: FixedHash::zero(),
            command_merkle_root: FixedHash::zero(),
            total_leader_fee: 0,
            // Not a dummy block
            signature: Some(SchnorrSignatureBytes::zero()),
            timestamp: EpochTime::now().as_u64(),
            epoch_hash: FixedHash::zero(),
            accumulated_data: ShardGroupAccumulatedData::default(),
            extra_data: ExtraData::new(),
            timeout_certificate_id: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn dummy_block(
        network: Network,
        protocol_version: ProtocolVersion,
        parent: BlockId,
        proposed_by: RistrettoPublicKeyBytes,
        height: NodeHeight,
        justify_id: PcId,
        epoch: Epoch,
        shard_group: ShardGroup,
        parent_state_merkle_root: FixedHash,
        parent_timestamp: u64,
        parent_epoch_hash: FixedHash,
        parent_accumulated_data: ShardGroupAccumulatedData,
    ) -> Self {
        let mut block = Self {
            id: BlockId::zero(),
            network,
            protocol_version,
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
            accumulated_data: parent_accumulated_data,
            extra_data: ExtraData::new(),
            timeout_certificate_id: None,
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

    /// The metadata hash is the one header field the base layer never recomputes: a commit proof carries it as an
    /// opaque value inside the header preimage that `tari_sidechain` hashes. Fields that only this network's
    /// validity rules read therefore commit here, which keeps the header preimage identical to the one the base
    /// layer verifies while still binding them into the block id.
    pub fn calculate_metadata_hash(&self) -> FixedHash {
        let fields = match self.protocol_version {
            ProtocolVersion::V0 => MetadataHashFields::V1(MetadataHashFieldsV1 {
                total_leader_fee: self.total_leader_fee,
                timestamp: self.timestamp,
                extra_data: &self.extra_data,
            }),
            ProtocolVersion::V1 => MetadataHashFields::V2(MetadataHashFieldsV2 {
                total_leader_fee: self.total_leader_fee,
                timestamp: self.timestamp,
                extra_data: &self.extra_data,
                timeout_certificate_id: self.timeout_certificate_id.as_ref().map(TcId::hash),
            }),
        };
        hashing::block_metadata_hasher().chain(&fields).finalize().into()
    }

    pub fn calculate_hash(&self) -> FixedHash {
        // This hash reduces proof sizes. A proof-of-commit only needs to include this hash and not
        // the data.
        let metadata_hash = self.calculate_metadata_hash();
        let accumulated_data = self.accumulated_data.into();

        let shard_group = tari_sidechain::ShardGroup {
            start: self.shard_group.start().as_u32(),
            end_inclusive: self.shard_group.end().as_u32(),
        };

        // This selection must stay identical to `tari_sidechain::SidechainBlockHeader::calculate_hash`, which is what
        // the base layer uses to verify a commit proof against the block ID a committee signed.
        let fields = match self.protocol_version {
            // Version 0 commits to a preimage that carries no version, so its block IDs stay reproducible.
            ProtocolVersion::V0 => BlockHeaderHashFields::V1(BlockHeaderHashFieldsV1 {
                network: self.network.as_byte(),
                justify_id: self.justify_id.hash(),
                height: self.height.as_u64(),
                epoch: self.epoch.as_u64(),
                epoch_hash: &self.epoch_hash,
                shard_group,
                proposed_by: self.proposed_by.as_bytes(),
                state_merkle_root: &self.state_merkle_root,
                command_merkle_root: &self.command_merkle_root,
                accumulated_data: &accumulated_data,
                metadata_hash: &metadata_hash,
            }),
            // From version 1 the version is part of the preimage, so that two versions sharing a preimage shape
            // still produce distinct block IDs and the version a block claims cannot be altered without
            // invalidating it.
            ProtocolVersion::V1 => BlockHeaderHashFields::V2(BlockHeaderHashFieldsV2 {
                network: self.network.as_byte(),
                protocol_version: self.protocol_version.as_u32(),
                justify_id: self.justify_id.hash(),
                height: self.height.as_u64(),
                epoch: self.epoch.as_u64(),
                epoch_hash: &self.epoch_hash,
                shard_group,
                proposed_by: self.proposed_by.as_bytes(),
                state_merkle_root: &self.state_merkle_root,
                command_merkle_root: &self.command_merkle_root,
                accumulated_data: &accumulated_data,
                metadata_hash: &metadata_hash,
            }),
        };

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

    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    pub fn parent(&self) -> &BlockId {
        &self.parent
    }

    pub fn justify_id(&self) -> &PcId {
        &self.justify_id
    }

    pub fn timeout_certificate_id(&self) -> Option<&TcId> {
        self.timeout_certificate_id.as_ref()
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

    pub fn accumulated_data(&self) -> &ShardGroupAccumulatedData {
        &self.accumulated_data
    }

    pub fn total_accumulated_exhaust_burn(&self) -> u128 {
        self.accumulated_data.total_exhaust_burn
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
    V2(MetadataHashFieldsV2<'a>),
}

#[derive(Debug, BorshSerialize)]
struct MetadataHashFieldsV1<'a> {
    total_leader_fee: u64,
    timestamp: u64,
    extra_data: &'a ExtraData,
}

#[derive(Debug, BorshSerialize)]
struct MetadataHashFieldsV2<'a> {
    total_leader_fee: u64,
    timestamp: u64,
    extra_data: &'a ExtraData,
    timeout_certificate_id: Option<&'a FixedHash>,
}

#[cfg(test)]
mod tests {
    use tari_consensus_types::ProposalCertificate;

    use super::*;

    fn header(protocol_version: ProtocolVersion) -> BlockHeader {
        header_with_timeout_certificate(protocol_version, None)
    }

    fn header_with_timeout_certificate(
        protocol_version: ProtocolVersion,
        timeout_certificate_id: Option<TcId>,
    ) -> BlockHeader {
        let shard_group = ShardGroup::all_shards(NumPreshards::P64);
        BlockHeader::create(
            Network::LocalNet,
            protocol_version,
            BlockId::zero(),
            ProposalCertificate::genesis(Epoch(1), shard_group).calculate_id(),
            timeout_certificate_id,
            NodeHeight(2),
            Epoch(1),
            shard_group,
            RistrettoPublicKeyBytes::default(),
            FixedHash::zero(),
            &BTreeSet::new(),
            1,
            SchnorrSignatureBytes::zero(),
            1234,
            FixedHash::zero(),
            ShardGroupAccumulatedData::default(),
            ExtraData::new(),
        )
        .unwrap()
    }

    /// The encoding of a header that carries no protocol version: the same array truncated before element 16,
    /// which is `protocol_version`, dropping it and every field added after it.
    fn encode_without_protocol_version(header: &BlockHeader) -> Vec<u8> {
        const PROTOCOL_VERSION_INDEX: u64 = 16;
        let bytes = tari_bor::encode(header).unwrap();
        let mut decoder = minicbor::Decoder::new(&bytes);
        let len = decoder
            .array()
            .unwrap()
            .expect("BlockHeader encodes as a definite length array");
        assert!(len > PROTOCOL_VERSION_INDEX);
        let body_start = decoder.position();
        for _ in 0..PROTOCOL_VERSION_INDEX {
            decoder.skip().unwrap();
        }
        let protocol_version_start = decoder.position();

        let mut out = Vec::new();
        minicbor::Encoder::new(&mut out).array(PROTOCOL_VERSION_INDEX).unwrap();
        out.extend_from_slice(&bytes[body_start..protocol_version_start]);
        out
    }

    #[test]
    fn a_header_encoded_without_a_protocol_version_decodes_as_v0() {
        let header = header(ProtocolVersion::V0);
        let decoded: BlockHeader = tari_bor::decode(&encode_without_protocol_version(&header)).unwrap();

        assert_eq!(decoded.protocol_version(), ProtocolVersion::V0);
        assert_eq!(decoded.id(), header.id());
        assert_eq!(decoded.calculate_hash(), header.calculate_hash());
    }

    #[test]
    fn a_header_round_trips_its_protocol_version() {
        for protocol_version in [ProtocolVersion::V0, ProtocolVersion::V1] {
            let header = header(protocol_version);
            let bytes = tari_bor::encode(&header).unwrap();
            let decoded: BlockHeader = tari_bor::decode(&bytes).unwrap();
            assert_eq!(decoded.protocol_version(), protocol_version);
            assert_eq!(decoded.calculate_hash(), header.calculate_hash());
        }
    }

    #[test]
    fn a_header_round_trips_its_timeout_certificate_id() {
        let tc_id = TcId::from([7u8; 32]);
        let header = header_with_timeout_certificate(ProtocolVersion::V1, Some(tc_id));
        let decoded: BlockHeader = tari_bor::decode(&tari_bor::encode(&header).unwrap()).unwrap();
        assert_eq!(decoded.timeout_certificate_id(), Some(&tc_id));
        assert_eq!(decoded.id(), header.id());
    }

    #[test]
    fn from_v1_the_timeout_certificate_id_is_in_the_block_id() {
        let without = header(ProtocolVersion::V1);
        let with = header_with_timeout_certificate(ProtocolVersion::V1, Some(TcId::from([7u8; 32])));
        let with_other = header_with_timeout_certificate(ProtocolVersion::V1, Some(TcId::from([8u8; 32])));
        assert_ne!(without.calculate_metadata_hash(), with.calculate_metadata_hash());
        assert_ne!(with.calculate_metadata_hash(), with_other.calculate_metadata_hash());
        assert_ne!(without.id(), with.id());
        assert_ne!(with.id(), with_other.id());
    }

    #[test]
    fn v0_block_ids_do_not_commit_to_the_timeout_certificate_id() {
        let without = header(ProtocolVersion::V0);
        let with = header_with_timeout_certificate(ProtocolVersion::V0, Some(TcId::from([7u8; 32])));
        assert_eq!(without.id(), with.id());
    }

    #[test]
    fn each_protocol_version_hashes_a_header_differently() {
        assert_ne!(
            header(ProtocolVersion::V0).calculate_hash(),
            header(ProtocolVersion::V1).calculate_hash()
        );
    }
}
