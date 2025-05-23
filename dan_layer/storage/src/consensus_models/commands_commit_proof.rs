//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use tari_common_types::types::{CompressedPublicKey, FixedHash};
use tari_crypto::tari_utilities::ByteArray;
use tari_sidechain::{CommitProofElement, SidechainBlockCommitProof, SidechainProofValidationError};
use tari_state_tree::{compute_merkle_root_for_hashes, StateTreeError, TreeHash};
use tari_template_lib::prelude::RistrettoPublicKeyBytes;

use crate::consensus_models::Command;

pub type CheckVnFunc<'a> = dyn Fn(&RistrettoPublicKeyBytes) -> Result<bool, SidechainProofValidationError> + 'a;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum CommandsCommitProof {
    V1(CommandsCommitProofV1),
}

impl CommandsCommitProof {
    pub fn new_latest(commands: Vec<CommandOrHash>, commit_proof: SidechainBlockCommitProof) -> Self {
        Self::new_v1(commands, commit_proof)
    }

    pub fn new_v1(commands: Vec<CommandOrHash>, commit_proof: SidechainBlockCommitProof) -> Self {
        Self::V1(CommandsCommitProofV1 { commands, commit_proof })
    }

    pub fn commands(&self) -> &[CommandOrHash] {
        match self {
            Self::V1(proof) => proof.commands(),
        }
    }

    pub fn applicable_commands_iter(&self) -> impl Iterator<Item = &Command> + '_ {
        self.commands().iter().filter_map(|cmd| cmd.command())
    }

    pub fn sidechain_block_commit_proof(&self) -> &SidechainBlockCommitProof {
        match self {
            Self::V1(proof) => proof.commit_proof(),
        }
    }

    pub fn calculate_block_id(&self) -> FixedHash {
        match self {
            Self::V1(proof) => proof.commit_proof().header.calculate_block_id(),
        }
    }

    pub fn validate_header(
        &self,
        expected_proposed: &RistrettoPublicKeyBytes,
    ) -> Result<(), ForeignProposalCommitProofError> {
        match self {
            Self::V1(proof) => proof.validate_header(expected_proposed),
        }
    }

    pub fn validate_committed(
        &self,
        quorum_threshold: usize,
        check_vn: &CheckVnFunc<'_>,
    ) -> Result<(), ForeignProposalCommitProofError> {
        match self {
            Self::V1(proof) => proof.validate_committed(quorum_threshold, check_vn),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct CommandsCommitProofV1 {
    pub commands: Vec<CommandOrHash>,
    pub commit_proof: SidechainBlockCommitProof,
}

impl CommandsCommitProofV1 {
    pub fn commands(&self) -> &[CommandOrHash] {
        &self.commands
    }

    pub fn commit_proof(&self) -> &SidechainBlockCommitProof {
        &self.commit_proof
    }

    pub fn validate_committed(
        &self,
        quorum_threshold: usize,
        check_vn: &CheckVnFunc<'_>,
    ) -> Result<(), ForeignProposalCommitProofError> {
        self.validate_command_merkle_root()?;
        self.commit_proof
            .validate_committed(quorum_threshold, &|pk: &CompressedPublicKey| {
                check_vn(&RistrettoPublicKeyBytes::from_bytes(pk.as_bytes()).expect("already checked"))
            })?;
        Ok(())
    }

    pub fn validate_header(
        &self,
        _expected_proposer: &RistrettoPublicKeyBytes,
    ) -> Result<(), ForeignProposalCommitProofError> {
        self.validate_well_formed()?;
        let header = &self.commit_proof.header;
        if header.height == 0 {
            return Err(ForeignProposalCommitProofError::Invalid(anyhow::anyhow!(
                "Header height is 0"
            )));
        }

        let signature = header
            .signature
            .to_schnorr_signature()
            .map_err(|e| ForeignProposalCommitProofError::Invalid(anyhow!("Malformed signature: {e}")))?;
        let block_id = header.calculate_block_id();
        // TODO: we currently cannot determine the correct leader from the proof data
        // if header.proposed_by.as_bytes() != expected_proposer.as_bytes() {
        //     return Err(ForeignProposalCommitProofError::Invalid(anyhow::anyhow!(
        //         "Proposed by public key {} does not match expected proposer {}",
        //         header.proposed_by,
        //         expected_proposer
        //     )));
        // }

        let proposed_by = header
            .proposed_by
            .to_public_key()
            .map_err(|e| ForeignProposalCommitProofError::Invalid(anyhow!("Malformed proposer public key: {e}")))?;
        if !signature.verify(&proposed_by, block_id) {
            return Err(ForeignProposalCommitProofError::Invalid(anyhow::anyhow!(
                "Signature verification failed for block id: {}",
                block_id
            )));
        }
        Ok(())
    }

    fn validate_command_merkle_root(&self) -> Result<(), ForeignProposalCommitProofError> {
        // We essentially rebuild the tree here to check the Merkle root. Since we expect fewer than 1000
        // commands, this is probably acceptable. We don't have a Merkle proof that supports multiple non-consecutive
        // leaves. Such a proof would carry on overhead when compared to a Merkle proof for a single leaf. This
        // is because the sparse single-leaf proofs rely on depth-ordered siblings (i.e. siblings vec + the leaf
        // key implicitly "encode" the hash ordering for proof verification) and therefore we'd need some other way to
        // represent this ordering within a multi-proof with nonduplicate nodes. Since we only include the full command
        // data for applicable commands, such a multi-proof may not be worthwhile.
        let command_hashes = self.commands.iter().map(|cmd| TreeHash::new(cmd.hash().into_array()));
        let root_hash = compute_merkle_root_for_hashes(command_hashes)?;
        if FixedHash::from(root_hash.into_array()) != self.commit_proof.header.command_merkle_root {
            return Err(ForeignProposalCommitProofError::InvalidCommandMerkleRoot {
                calculated: root_hash,
                expected: self.commit_proof.header.command_merkle_root,
            });
        }
        Ok(())
    }

    fn validate_well_formed(&self) -> Result<(), ForeignProposalCommitProofError> {
        const MAX_COMMANDS: usize = 1000;

        if self.commands.is_empty() {
            return Err(ForeignProposalCommitProofError::Invalid(anyhow::anyhow!(
                "CommandsCommitProofV1 must have at least one command"
            )));
        }

        if self.commands.len() > MAX_COMMANDS {
            return Err(ForeignProposalCommitProofError::Invalid(anyhow::anyhow!(
                "CommandsCommitProofV1 must have at most 1000 commands but has {}",
                self.commands.len()
            )));
        }

        // Since CompressedPublicKey is a variable length vec, we make double sure that the length is correct.
        if self
            .commit_proof
            .header
            .signature
            .get_compressed_public_nonce()
            .as_bytes()
            .len() !=
            RistrettoPublicKeyBytes::length()
        {
            return Err(ForeignProposalCommitProofError::Invalid(anyhow::anyhow!(
                "Signature public nonce is not a valid RistrettoPublicKey"
            )));
        }
        if self.commit_proof.header.proposed_by.as_bytes().len() != RistrettoPublicKeyBytes::length() {
            return Err(ForeignProposalCommitProofError::Invalid(anyhow::anyhow!(
                "Proposed by public key is not a valid RistrettoPublicKey"
            )));
        }
        let last_qc = self
            .commit_proof
            .proof_elements
            .last()
            .and_then(|elem| {
                if let CommitProofElement::QuorumCertificate(qc) = elem {
                    Some(qc)
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                ForeignProposalCommitProofError::Invalid(anyhow::anyhow!(
                    "Last element in commit proof is not a QuorumCertificate"
                ))
            })?;
        if last_qc.calculate_justified_block() != self.commit_proof.header.calculate_block_id() {
            return Err(ForeignProposalCommitProofError::Invalid(anyhow::anyhow!(
                "Last QuorumCertificate does not justify the block id"
            )));
        }

        for (i, elem) in self.commit_proof.proof_elements.iter().enumerate() {
            match elem {
                CommitProofElement::QuorumCertificate(qc) => {
                    if qc.signatures.is_empty() {
                        return Err(ForeignProposalCommitProofError::Invalid(anyhow::anyhow!(
                            "QuorumCertificate at index {i} has no signatures"
                        )));
                    }
                    if qc
                        .signatures
                        .iter()
                        .any(|s| s.public_key.as_bytes().len() != RistrettoPublicKeyBytes::length())
                    {
                        return Err(ForeignProposalCommitProofError::Invalid(anyhow::anyhow!(
                            "QuorumCertificate at index {i} has invalid public key"
                        )));
                    }
                },
                CommitProofElement::ChainLinks(links) => {
                    if links.is_empty() {
                        return Err(ForeignProposalCommitProofError::Invalid(anyhow::anyhow!(
                            "ChainLinks at index {i} is empty"
                        )));
                    }
                },
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum CommandOrHash {
    /// The full command data
    Command(Command),
    /// A hash of a command
    Hash(FixedHash),
}

impl CommandOrHash {
    pub fn hash(&self) -> FixedHash {
        match self {
            Self::Command(cmd) => cmd.hash(),
            Self::Hash(hash) => *hash,
        }
    }

    pub fn command(&self) -> Option<&Command> {
        match self {
            Self::Command(cmd) => Some(cmd),
            Self::Hash(_) => None,
        }
    }

    pub fn into_command(self) -> Option<Command> {
        match self {
            Self::Command(cmd) => Some(cmd),
            Self::Hash(_) => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ForeignProposalCommitProofError {
    #[error("State tree error: {0}")]
    StateTreeError(#[from] StateTreeError),
    #[error("Invalid command Merkle root. Calculated: {calculated}, expected: {expected}")]
    InvalidCommandMerkleRoot { calculated: TreeHash, expected: FixedHash },
    #[error("Sidechain Proof Validation Error: {0}")]
    SidechainProofValidationError(#[from] SidechainProofValidationError),
    #[error("The foreign proposal was invalid: {0}")]
    Invalid(#[from] anyhow::Error),
}
