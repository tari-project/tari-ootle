//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use blake2::{Blake2b, digest::consts::U32};
use log::*;
use ootle_byte_type::ConvertFromByteType;
use tari_common_types::types::{CompressedCommitment, CompressedPublicKey, CompressedSignature, FixedHash};
use tari_crypto::{
    commitment::HomomorphicCommitmentFactory,
    ristretto::{RistrettoSchnorr, RistrettoSecretKey, pedersen::PedersenCommitment},
    tari_utilities::ByteArray,
};
use tari_engine::traits::ClaimProofVerifier;
use tari_engine_types::{confidential::MinotariBurnClaimProof, crypto::get_commitment_factory};
use tari_hashing::{TransactionHashDomain, hashers::KernelMmrHasherBlake256};
use tari_mmr::common::LeafIndex;
use tari_ootle_common_types::{
    Epoch,
    base_layer_hashing::ownership_proof_hasher64,
    optional::{IsNotFoundError, Optional},
};
use tari_ootle_storage::global::{GlobalDb, GlobalDbAdapter};
use tari_ootle_transaction::Network;
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;
use tari_transaction_components::{
    consensus::DomainSeparatedConsensusHasher,
    transaction_components::{KernelFeatures, TransactionKernel},
};

const LOG_TARGET: &str = "tari::ootle::claim_burn_proof_verifier";

pub struct TariClaimBurnProofVerifier<TGlobalBackend> {
    knowledge_proof: KnowledgeProofVerifier,
    kernel_merkle_proof: KernelMerkleProofVerifier<TGlobalBackend>,
}

impl<TGlobalBackend> TariClaimBurnProofVerifier<TGlobalBackend> {
    pub fn new(network: Network, global_db: GlobalDb<TGlobalBackend>) -> Self {
        Self {
            knowledge_proof: KnowledgeProofVerifier { network },
            kernel_merkle_proof: KernelMerkleProofVerifier { global_db, network },
        }
    }
}

impl<TGlobalBackend> ClaimProofVerifier for TariClaimBurnProofVerifier<TGlobalBackend>
where
    TGlobalBackend: GlobalDbAdapter,
    TGlobalBackend::Error: Display + IsNotFoundError,
{
    fn verify_claim_proof(
        &self,
        epoch: Epoch,
        claimant: &RistrettoPublicKeyBytes,
        claim_proof: &MinotariBurnClaimProof,
    ) -> Result<(), String> {
        // 1. Verify proof of knowledge of the burn commitment opening
        self.knowledge_proof.verify_claim_proof(epoch, claimant, claim_proof)?;
        // 2. Verify kernel inclusion proof
        self.kernel_merkle_proof
            .verify_claim_proof(epoch, claimant, claim_proof)?;
        // Claim proof is valid
        Ok(())
    }
}

pub struct KernelMerkleProofVerifier<TGlobalBackend> {
    global_db: GlobalDb<TGlobalBackend>,
    network: Network,
}

impl<TGlobalBackend> KernelMerkleProofVerifier<TGlobalBackend> {
    pub fn new(global_db: GlobalDb<TGlobalBackend>, network: Network) -> Self {
        Self { global_db, network }
    }
}

impl<TGlobalBackend> KernelMerkleProofVerifier<TGlobalBackend> {
    fn hash_kernel(&self, kernel: &TransactionKernel) -> FixedHash {
        // Ye olde CURRENT_NETWORK global means we reimplement kernel.hash() here
        DomainSeparatedConsensusHasher::<TransactionHashDomain, Blake2b<U32>>::new_with_network(
            "transaction_kernel",
            self.network.as_byte(),
        )
        .chain(kernel)
        .finalize()
        .into()
    }
}

impl<TGlobalBackend> ClaimProofVerifier for KernelMerkleProofVerifier<TGlobalBackend>
where
    TGlobalBackend: GlobalDbAdapter,
    TGlobalBackend::Error: Display + IsNotFoundError,
{
    fn verify_claim_proof(
        &self,
        epoch: Epoch,
        _claimant: &RistrettoPublicKeyBytes,
        claim_proof: &MinotariBurnClaimProof,
    ) -> Result<(), String> {
        // 1. Decode the merkle proof
        let (proof, read) = bincode::serde::decode_from_slice::<tari_mmr::MerkleProof, _>(
            claim_proof.encoded_merkle_proof.encoded_merkle_proof.as_slice(),
            // L1 uses bincode v1
            bincode::config::legacy(),
        )
        .map_err(|e| {
            warn!(target: LOG_TARGET, "Claim burn failed - malformed merkle proof: {}", e);
            format!("malformed merkle proof: {}", e)
        })?;
        if read != claim_proof.encoded_merkle_proof.encoded_merkle_proof.len() {
            warn!(target: LOG_TARGET, "Claim burn failed - malformed merkle proof: read length mismatch");
            return Err("malformed merkle proof: read length mismatch".to_string());
        }

        // 2. Fetch the block header for this proof
        let block_header = {
            let mut tx = self.global_db.create_transaction().map_err(|e| {
                warn!(target: LOG_TARGET, "Claim burn failed - could not create DB transaction: {}", e);
                format!("could not create DB transaction: {}", e)
            })?;
            self.global_db
                .block_headers(&mut tx)
                .get_by_hash(epoch, &claim_proof.encoded_merkle_proof.block_hash)
                .optional()
                .map_err(|e| {
                    warn!(target: LOG_TARGET, "Claim burn failed - could not fetch block header: {}", e);
                    format!("could not fetch block header: {}", e)
                })?
        };
        let block_header = block_header.ok_or_else(|| {
            warn!(
                target: LOG_TARGET,
                "Claim burn failed - block header not found for hash {} in epoch {}",
                claim_proof.encoded_merkle_proof.block_hash, epoch
            );
            format!(
                "block header not found for hash {}. The claim may be invalid, or the burn may have occurred after \
                 the current epoch, and therefore is not yet claimable.",
                claim_proof.encoded_merkle_proof.block_hash
            )
        })?;

        // 3. Reconstitute the kernel to get the hash
        let kernel = &claim_proof.kernel;
        let kernel = TransactionKernel {
            version: kernel
                .version
                .try_into()
                .map_err(|e| format!("bad kernel version: {}", e))?,
            features: KernelFeatures::BURN_KERNEL,
            fee: kernel.fee.into(),
            lock_height: kernel.lock_height,
            excess: CompressedCommitment::from_canonical_bytes(kernel.excess.as_bytes()).map_err(|e| {
                warn!(target: LOG_TARGET, "Claim burn failed - malformed excess commitment: {}", e);
                format!("malformed excess commitment: {}", e)
            })?,
            excess_sig: CompressedSignature::new(
                CompressedPublicKey::from_canonical_bytes(kernel.excess_sig.public_nonce().as_bytes())
                    .map_err(|e| format!("malformed excess signature nonce: {}", e))?,
                RistrettoSecretKey::from_canonical_bytes(kernel.excess_sig.signature().as_bytes())
                    .map_err(|e| format!("malformed excess signature: {}", e))?,
            ),
            burn_commitment: Some(
                CompressedCommitment::from_canonical_bytes(claim_proof.commitment.as_bytes()).map_err(|e| {
                    warn!(target: LOG_TARGET, "Claim burn failed - malformed burn commitment: {}", e);
                    format!("malformed burn commitment: {}", e)
                })?,
            ),
        };

        // 4. Verify the merkle proof (proving that the kernel is in the block)
        let leaf_index = claim_proof.encoded_merkle_proof.leaf_index.try_into().map_err(|e| {
            warn!(target: LOG_TARGET, "Claim burn failed - invalid leaf index: {}", e);
            format!("invalid leaf index: {}", e)
        })?;

        let kernel_hash = self.hash_kernel(&kernel);

        proof
            .verify_leaf::<KernelMmrHasherBlake256>(
                block_header.kernel_merkle_root.as_slice(),
                kernel_hash.as_slice(),
                LeafIndex(leaf_index),
            )
            .map_err(|e| {
                warn!(target: LOG_TARGET, "Claim burn failed - invalid merkle proof: {}", e);
                format!("invalid merkle proof: {}", e)
            })?;

        Ok(())
    }
}

pub struct KnowledgeProofVerifier {
    network: Network,
}

impl KnowledgeProofVerifier {
    pub fn new(network: Network) -> Self {
        Self { network }
    }
}

impl ClaimProofVerifier for KnowledgeProofVerifier {
    fn verify_claim_proof(
        &self,
        _epoch: Epoch,
        claimant: &RistrettoPublicKeyBytes,
        claim: &MinotariBurnClaimProof,
    ) -> Result<(), String> {
        let MinotariBurnClaimProof {
            commitment,
            ownership_proof: proof_of_knowledge,
            value,
            ..
        } = claim;

        // `claimant` is the stealth claim public key `C = H(r·P)·G + P`. The runtime supplies it
        // via `seal_signer_public_key`: the L2 wallet signs the claim transaction with
        // `s = H(R·p) + p`, so the transaction's seal-signer pubkey is `s·G = C`.
        // NOTE: .as_bytes() used because the tari_crypto borsh implementations serialize fixed length bytes as variable
        // length bytes of size 32
        let message = ownership_proof_hasher64(self.network)
            .chain(&commitment.as_bytes())
            .chain(&claimant.as_bytes())
            .finalize();

        let commitment = PedersenCommitment::convert_from_byte_type(commitment).map_err(|e| {
            warn!(target: LOG_TARGET, "Claim burn failed - malformed commitment: {}", e);
            format!("malformed commitment: {}", e)
        })?;

        let proof_of_knowledge = RistrettoSchnorr::convert_from_byte_type(proof_of_knowledge).map_err(|e| {
            warn!(target: LOG_TARGET, "Claim burn failed - malformed proof of knowledge: {}", e);
            format!("malformed proof of knowledge: {}", e)
        })?;

        let value_commit = get_commitment_factory().commit_value(&RistrettoSecretKey::default(), *value);
        // k.G = C - v.H
        let signer_pk = commitment.as_public_key() - value_commit.as_public_key();

        if !proof_of_knowledge.verify(&signer_pk, message) {
            warn!(target: LOG_TARGET, "Claim burn failed - signature verification failed");
            return Err("invalid proof of knowledge signature".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{
        commitment::HomomorphicCommitmentFactory,
        keys::{PublicKey as _, SecretKey as _},
        ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
    };
    use tari_engine::traits::ClaimProofVerifier;
    use tari_engine_types::{
        confidential::{AbridgedTransactionKernel, EncodedMerkleProof, MinotariBurnClaimProof},
        crypto::get_commitment_factory,
    };
    use tari_ootle_common_types::{Epoch, base_layer_hashing::ownership_proof_hasher64};
    use tari_ootle_transaction::Network;
    use tari_template_lib::types::crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, SchnorrSignatureBytes};

    use super::KnowledgeProofVerifier;

    /// Mints a `MinotariBurnClaimProof` whose `ownership_proof` Schnorr signature is bound to
    /// `claimant_pk` (the key the message commits to in `H(commitment ‖ claimant_pk)`).
    fn build_proof(network: Network, value: u64, claimant_pk: RistrettoPublicKeyBytes) -> MinotariBurnClaimProof {
        let mask = RistrettoSecretKey::random(&mut rand::rng());
        let commitment = get_commitment_factory().commit_value(&mask, value);
        let commitment_bytes = commitment.to_byte_type();

        let message = ownership_proof_hasher64(network)
            .chain(&commitment_bytes.as_bytes())
            .chain(&claimant_pk.as_bytes())
            .finalize();
        let signature = RistrettoSchnorr::sign(&mask, &message[..], &mut rand::rng()).expect("sign with random nonce");

        // Dummy merkle proof / kernel — KnowledgeProofVerifier doesn't read them.
        let encoded_merkle_proof = EncodedMerkleProof {
            block_hash: Default::default(),
            encoded_merkle_proof: bounded_vec::BoundedVec::<u8, 1, 4096>::from_vec(vec![0]).expect("valid bounded vec"),
            leaf_index: 0,
        };
        let kernel = AbridgedTransactionKernel {
            version: 0,
            fee: 0,
            lock_height: 0,
            excess: PedersenCommitmentBytes::zero(),
            excess_sig: SchnorrSignatureBytes::zero(),
        };

        MinotariBurnClaimProof {
            burn_public_key: RistrettoPublicKeyBytes::zero(),
            commitment: commitment_bytes,
            ownership_proof: signature.to_byte_type(),
            encoded_merkle_proof,
            kernel,
            value,
            sender_offset_public_key: RistrettoPublicKeyBytes::zero(),
        }
    }

    #[test]
    fn verifies_against_caller_supplied_claimant() {
        let network = Network::LocalNet;
        // The runtime feeds the transaction's seal-signer pubkey as the claimant. For stealth
        // claims the wallet signs with `s = H(R·p) + p`, so this is `C = s·G`.
        let (_c_sec, c_pub) = RistrettoPublicKey::random_keypair(&mut rand::rng());

        let proof = build_proof(network, 2_000, c_pub.to_byte_type());
        let verifier = KnowledgeProofVerifier::new(network);

        verifier
            .verify_claim_proof(Epoch(0), &c_pub.to_byte_type(), &proof)
            .expect("proof should verify against the seal signer C");
    }

    #[test]
    fn rejects_when_claimant_does_not_match_signed_binding() {
        let network = Network::LocalNet;
        let (_c_sec, c_pub) = RistrettoPublicKey::random_keypair(&mut rand::rng());
        let (_wrong_sec, wrong_pub) = RistrettoPublicKey::random_keypair(&mut rand::rng());

        // Signed against c_pub but the runtime passes wrong_pub.
        let proof = build_proof(network, 3_000, c_pub.to_byte_type());

        let verifier = KnowledgeProofVerifier::new(network);
        let result = verifier.verify_claim_proof(Epoch(0), &wrong_pub.to_byte_type(), &proof);
        assert!(result.is_err(), "expected verification to reject mismatched claimant");
    }
}
