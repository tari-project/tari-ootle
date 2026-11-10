//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use borsh::BorshSerialize;
use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_ootle_common_types::{Epoch, NodeHeight, ShardGroup, hashing::quorum_certificate_id_hasher};
use tari_sidechain::{FixedHash, QcSignatures, QuorumDecision, SignerBitmap};

use crate::{
    bookkeeping::{HighPc, LeafBlock},
    ids::{BlockId, PcId},
};

#[derive(Debug, Clone, Deserialize, Serialize, BorshSerialize, Encode, Decode, CborLen)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ProposalCertificate {
    #[n(0)]
    height: NodeHeight,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[serde(with = "ootle_serde::hex")]
    // FixedHash is from tari_common_types (external) — bridge through serde.
    #[n(1)]
    #[cbor(with = "tari_bor::adapters::serde_bridge")]
    header_hash: FixedHash,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[n(2)]
    parent_id: BlockId,
    #[n(3)]
    epoch: Epoch,
    #[n(4)]
    shard_group: ShardGroup,
    #[cfg_attr(
        feature = "ts",
        ts(type = "{ Schnorr: { signers: string, signatures: Array<{ public_nonce: string, signature: string }> } }")
    )]
    // QcSignatures is from tari_sidechain (external) — bridge through serde.
    #[n(5)]
    #[cbor(with = "tari_bor::adapters::serde_bridge")]
    signatures: QcSignatures,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    // QuorumDecision is from tari_sidechain (external) — bridge through serde.
    #[n(6)]
    #[cbor(with = "tari_bor::adapters::serde_bridge")]
    decision: QuorumDecision,
}

impl ProposalCertificate {
    pub fn new(
        header_hash: FixedHash,
        parent_id: BlockId,
        height: NodeHeight,
        epoch: Epoch,
        shard_group: ShardGroup,
        signatures: QcSignatures,
        decision: QuorumDecision,
    ) -> Self {
        Self {
            header_hash,
            parent_id,
            height,
            epoch,
            shard_group,
            signatures,
            decision,
        }
    }

    pub fn genesis(epoch: Epoch, shard_group: ShardGroup) -> Self {
        Self {
            header_hash: FixedHash::zero(),
            parent_id: BlockId::zero(),
            height: NodeHeight::zero(),
            epoch,
            shard_group,
            signatures: Self::genesis_signatures(),
            decision: QuorumDecision::Accept,
        }
    }

    /// The signatures of the certificate that justifies an epoch's zero block, which every node derives without a
    /// vote: no signers, for a committee of no members.
    pub fn genesis_signatures() -> QcSignatures {
        QcSignatures::Schnorr {
            signers: SignerBitmap::default(),
            signatures: vec![],
        }
    }

    /// Returns the hash of the QC. This is used to identify the QC and not for any secure purposes.
    /// However, we implement a secure hash (as opposed to a cheaper, non-collision-resistant hash e.g. siphash) to
    /// avoid any collision issues e.g. storage keys.
    pub fn calculate_id(&self) -> PcId {
        quorum_certificate_id_hasher()
            .chain(&self.header_hash)
            .chain(&self.parent_id)
            .chain(&self.signatures)
            .chain(&self.decision)
            .finalize_into_array()
            .into()
    }
}

impl ProposalCertificate {
    /// Returns `true` if this certificate has the canonical genesis shape that justifies an epoch's zero block
    /// (`height == 0`, `parent_id.is_zero()`, `header_hash == FixedHash::zero()`, `signatures == genesis_signatures()`,
    /// and `decision.is_accept()`).
    pub fn justifies_zero_block(&self) -> bool {
        self.height.is_zero() &&
            self.parent_id.is_zero() &&
            self.header_hash == FixedHash::zero() &&
            self.signatures == Self::genesis_signatures() &&
            self.decision.is_accept()
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    pub fn shard_group(&self) -> ShardGroup {
        self.shard_group
    }

    pub fn signatures(&self) -> &QcSignatures {
        &self.signatures
    }

    pub fn height(&self) -> NodeHeight {
        self.height
    }

    pub fn decision(&self) -> QuorumDecision {
        self.decision
    }

    pub fn calculate_block_id(&self) -> BlockId {
        BlockId::from_parent_and_header_hash(&self.parent_id, &self.header_hash)
    }

    pub fn header_hash(&self) -> &FixedHash {
        &self.header_hash
    }

    pub fn parent_id(&self) -> &BlockId {
        &self.parent_id
    }

    pub fn as_high_pc(&self) -> HighPc {
        HighPc {
            block_id: self.calculate_block_id(),
            block_height: self.height,
            epoch: self.epoch,
            qc_id: self.calculate_id(),
        }
    }

    pub fn as_leaf_block(&self) -> LeafBlock {
        LeafBlock {
            block_id: self.calculate_block_id(),
            height: self.height,
            epoch: self.epoch,
            shard_group: self.shard_group,
        }
    }
}

impl Display for ProposalCertificate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ProposalCertificate(block: {} {}, qc_id: {}, epoch: {}, {} signatures)",
            self.height,
            self.calculate_block_id(),
            self.calculate_id(),
            self.epoch,
            self.signatures.num_signatures()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shard_group() -> ShardGroup {
        ShardGroup::new(0, 255)
    }

    #[test]
    fn justifies_zero_block_requires_canonical_genesis_shape() {
        let genesis = ProposalCertificate::genesis(Epoch(3), shard_group());
        assert!(genesis.justifies_zero_block());
        assert!(genesis.calculate_block_id().is_zero());

        // Non-zero height with zero header hash is not a genesis certificate.
        let non_zero_height = ProposalCertificate::new(
            FixedHash::zero(),
            BlockId::zero(),
            NodeHeight(5),
            Epoch(3),
            shard_group(),
            ProposalCertificate::genesis_signatures(),
            QuorumDecision::Accept,
        );
        assert!(!non_zero_height.justifies_zero_block());

        // Non-zero parent_id with zero header hash is not a genesis certificate.
        let non_zero_parent = ProposalCertificate::new(
            FixedHash::zero(),
            BlockId::new(FixedHash::from([1u8; 32])),
            NodeHeight::zero(),
            Epoch(3),
            shard_group(),
            ProposalCertificate::genesis_signatures(),
            QuorumDecision::Accept,
        );
        assert!(!non_zero_parent.justifies_zero_block());
        assert!(!non_zero_parent.calculate_block_id().is_zero());

        // Reject decision is not a genesis certificate.
        let reject = ProposalCertificate::new(
            FixedHash::zero(),
            BlockId::zero(),
            NodeHeight::zero(),
            Epoch(3),
            shard_group(),
            ProposalCertificate::genesis_signatures(),
            QuorumDecision::Reject,
        );
        assert!(!reject.justifies_zero_block());

        // Non-empty signer bitmap / signatures with zero header_hash, height, and parent_id is not a genesis
        // certificate.
        let non_empty_sigs = ProposalCertificate::new(
            FixedHash::zero(),
            BlockId::zero(),
            NodeHeight::zero(),
            Epoch(3),
            shard_group(),
            QcSignatures::Schnorr {
                signers: SignerBitmap::from_bytes(vec![1]),
                signatures: vec![],
            },
            QuorumDecision::Accept,
        );
        assert!(!non_empty_sigs.justifies_zero_block());
    }
}
