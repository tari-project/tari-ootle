//   Copyright 2022 The Tari Project
//   SPDX-License-Identifier: BSD-3-clause

pub use tari_hashing::layer2::{
    block_hasher,
    block_metadata_hasher,
    command_hasher,
    proposal_vote_signature_hasher,
    tari_consensus_hasher,
};
use tari_hashing::layer2::{TariConsensusHasher, ValidatorNodeBmtHasherBlake2b};
use tari_mmr::{BalancedBinaryMerkleProof, BalancedBinaryMerkleTree, MergedBalancedBinaryMerkleProof};

pub type ValidatorNodeBalancedMerkleTree = BalancedBinaryMerkleTree<ValidatorNodeBmtHasherBlake2b>;
pub type ValidatorNodeMerkleProof = BalancedBinaryMerkleProof<ValidatorNodeBmtHasherBlake2b>;
pub type MergedValidatorNodeMerkleProof = MergedBalancedBinaryMerkleProof<ValidatorNodeBmtHasherBlake2b>;

pub fn quorum_certificate_id_hasher() -> TariConsensusHasher {
    tari_consensus_hasher("QuorumCertificateId")
}

pub fn timeout_certificate_id_hasher() -> TariConsensusHasher {
    tari_consensus_hasher("TimeoutCertificateId")
}
