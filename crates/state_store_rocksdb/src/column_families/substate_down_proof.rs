//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus_types::BlockId;
use tari_ootle_common_types::{SubstateAddress, shard::Shard};
use tari_ootle_storage::consensus_models::SubstateDownProofRecord;

use crate::{
    codecs::{BlockIdCodec, BytesCodec, FixedBytesCodec, KeyPrefix, SerdeBridgeCodec, ShardCodec},
    column_families::cf_names,
    prefixed,
    traits::Cf,
};

prefixed!(SubstateDownProofPrefix, KeyPrefix::SubstateDownProofs);

/// Per destroyed substate (keyed by its shard and the address of its `(id, version)`), the proof that it was
/// committed before it went down.
///
/// Records are not pruned with downed substate values: they are what keeps a destruction provable once the tree
/// nodes and blocks it was proved from are gone. Each holds a level-1 leaf proof and a level-2 shard-root proof, and
/// names the commit proof it relies on, stored once per block in [`DownProofCommitProofCf`]. The column grows with the
/// number of destroyed substate versions.
pub struct SubstateDownProofCf;

impl Cf for SubstateDownProofCf {
    type Key = (Shard, SubstateAddress);
    type KeyCodec = (ShardCodec, FixedBytesCodec<{ SubstateAddress::LENGTH }>);
    type Prefix = SubstateDownProofPrefix;
    type Value = SubstateDownProofRecord;
    type ValueCodec = SerdeBridgeCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::SUBSTATES
    }
}

prefixed!(DownProofCommitProofPrefix, KeyPrefix::DownProofCommitProofs);

/// The CBOR-encoded `CommittedBlockProof` of each block a [`SubstateDownProofCf`] record cites, stored once however
/// many records cite it.
///
/// An entry is shared by every record proved at that block, and records are deleted individually (a rewind deletes
/// the record of a substate it restores), so an entry is never deleted; retaining these is a follow-up. One entry is
/// written per block that commits a version some destroyed substate is proved up at, and its size grows with the
/// committee's signatures.
pub struct DownProofCommitProofCf;

impl Cf for DownProofCommitProofCf {
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
    type Prefix = DownProofCommitProofPrefix;
    type Value = Vec<u8>;
    type ValueCodec = BytesCodec;

    fn name() -> &'static str {
        cf_names::SUBSTATES
    }
}
