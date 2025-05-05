//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_common_types::types::CompressedPublicKey;
use tari_crypto::{ristretto::RistrettoSecretKey, tari_utilities::ByteArray};
use tari_dan_storage::{
    consensus_models::{Block, BlockHeader, EndOfEpochCommand, QuorumCertificate},
    StateStoreReadTransaction,
};
use tari_sidechain::{
    ChainLink,
    CommandCommitProof,
    CommitProofElement,
    EvictNodeAtom,
    EvictionProof,
    SidechainBlockCommitProof,
    SidechainBlockHeader,
    ValidatorBlockSignature,
    ValidatorQcSignature,
};
use tari_template_lib_types::crypto::SchnorrSignatureBytes;

use crate::hotstuff::HotStuffError;

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::eviction_proof";

pub fn generate_eviction_proofs<'a, TTx, I>(
    tx: &TTx,
    tip_qc: &QuorumCertificate,
    committed_blocks_with_evictions: I,
) -> Result<Vec<EvictionProof>, HotStuffError>
where
    TTx: StateStoreReadTransaction,
    I: IntoIterator<Item = &'a Block>,
    I::IntoIter: Clone,
{
    let evictions_iter = committed_blocks_with_evictions.into_iter();
    let num_evictions = evictions_iter.clone().map(|b| b.all_node_evictions().count()).sum();

    let mut proofs = Vec::with_capacity(num_evictions);
    for block in evictions_iter {
        // First generate a commit proof for the block which is shared by all EvictionProofs
        let block_commit_proof = generate_block_commit_proof(tx, tip_qc, block)?;

        for (idx, command) in block.commands().iter().enumerate() {
            let Some(atom) = command.evict_node() else {
                continue;
            };
            info!(target: LOG_TARGET, "🦶 Generating eviction proof for validator: {atom}");
            let inclusion_proof = block.compute_command_inclusion_proof(idx)?;
            let atom = EvictNodeAtom::new(
                CompressedPublicKey::from_canonical_bytes(atom.public_key.as_bytes()).map_err(|_| {
                    HotStuffError::InvariantError(format!(
                        "EvictNodeAtom RistrettoPublicKey non-canonical bytes for public key, in \
                         generate_eviction_proofs ({:?})",
                        atom.public_key,
                    ))
                })?,
            );
            let commit_command_proof = CommandCommitProof::new(atom, block_commit_proof.clone(), inclusion_proof);
            let proof = EvictionProof::new(commit_command_proof);
            proofs.push(proof);
        }
    }

    Ok(proofs)
}

pub fn generate_end_of_epoch_commit_proof<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    tip_qc: &QuorumCertificate,
    commit_block: &Block,
) -> Result<CommandCommitProof<EndOfEpochCommand>, HotStuffError> {
    if commit_block.commands().len() != 1 {
        return Err(HotStuffError::InvariantError(format!(
            "End of epoch block must have exactly one command, but found {}",
            commit_block.commands().len()
        )));
    }

    if !commit_block.is_epoch_end() {
        return Err(HotStuffError::InvariantError(format!(
            "Block is not an end-of-epoch block: {commit_block}"
        )));
    }

    let proof = generate_block_commit_proof(tx, tip_qc, commit_block)?;
    let inclusion_proof = commit_block.compute_command_inclusion_proof(0)?;
    let command_commit_proof = CommandCommitProof::new(EndOfEpochCommand, proof, inclusion_proof);
    Ok(command_commit_proof)
}

pub(crate) fn generate_block_commit_proof<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    commit_qc: &QuorumCertificate,
    commit_block: &Block,
) -> Result<SidechainBlockCommitProof, HotStuffError> {
    let mut proof_elements = Vec::with_capacity(3);

    if commit_block.is_dummy() || commit_block.signature().is_none() {
        return Err(HotStuffError::InvariantError(format!(
            "Commit block is a dummy block or has no signature in generate_block_commit_proof ({commit_block})",
        )));
    }

    debug!(target: LOG_TARGET, "Add commit_qc: {commit_qc}");
    proof_elements.push(convert_qc_to_proof_element(commit_qc)?);

    let mut block = commit_qc.get_block(tx)?;
    while block.id() != commit_block.id() {
        if block.justifies_parent() {
            debug!(target: LOG_TARGET, "Add justify: {}", block.justify());
            proof_elements.push(convert_qc_to_proof_element(block.justify())?);
            block = block.get_parent(tx)?;
        } else {
            block = block.get_parent(tx)?;
            let mut dummy_chain = vec![ChainLink {
                header_hash: block.header().calculate_hash(),
                parent_id: *block.parent().as_hash(),
            }];
            debug!(target: LOG_TARGET, "add dummy chain: {block}");
            let parent_id = *block.parent();
            let qc = block.into_justify();
            block = Block::get(tx, &parent_id)?;
            while block.id() != qc.block_id() {
                debug!(target: LOG_TARGET, "add dummy chain: {block} QC: {qc}");
                dummy_chain.push(ChainLink {
                    header_hash: block.header().calculate_hash(),
                    parent_id: *block.parent().as_hash(),
                });

                block = block.get_parent(tx)?;
                if block.height() < qc.block_height() {
                    return Err(HotStuffError::InvariantError(format!(
                        "Block height is less than the height of the QC in generate_block_commit_proof \
                         (block={block}, qc={qc})",
                    )));
                }
            }

            proof_elements.push(CommitProofElement::DummyChain(dummy_chain));
            debug!(target: LOG_TARGET, "Add justify: {}", qc);
            proof_elements.push(convert_qc_to_proof_element(&qc)?);
        }
        // Prevent possibility of endless loop
        if block.height() < commit_block.height() {
            return Err(HotStuffError::InvariantError(format!(
                "Block height is less than the commit block height in generate_block_commit_proof ({block}, \
                 commit_block={commit_block})",
            )));
        }
    }

    let command_commit_proof = SidechainBlockCommitProof {
        header: convert_block_to_sidechain_block_header(commit_block.header())?,
        proof_elements,
    };

    Ok(command_commit_proof)
}

pub fn convert_block_to_sidechain_block_header(header: &BlockHeader) -> Result<SidechainBlockHeader, HotStuffError> {
    // NOTE: if an invalid signature is not rejected prior to this, an invariant error will be caused by the block
    // proposer.
    let signature = convert_validator_block_signature(header.signature().expect("checked by caller"))?;

    Ok(SidechainBlockHeader {
        network: header.network().as_byte(),
        parent_id: *header.parent().as_hash(),
        justify_id: *header.justify_id().hash(),
        height: header.height().as_u64(),
        epoch: header.epoch().as_u64(),
        shard_group: tari_sidechain::ShardGroup {
            start: header.shard_group().start().as_u32(),
            end_inclusive: header.shard_group().end().as_u32(),
        },
        proposed_by: CompressedPublicKey::from_canonical_bytes(header.proposed_by().as_bytes()).map_err(|_| {
            HotStuffError::InvariantError(format!(
                "RistrettoPublicKey non-canonical bytes for proposed_by, in convert_block_to_sidechain_block_header \
                 ({})",
                header.proposed_by(),
            ))
        })?,
        state_merkle_root: *header.state_merkle_root(),
        command_merkle_root: *header.command_merkle_root(),
        metadata_hash: header.calculate_metadata_hash(),
        signature,
    })
}

fn convert_qc_to_proof_element(qc: &QuorumCertificate) -> Result<CommitProofElement, HotStuffError> {
    Ok(CommitProofElement::QuorumCertificate(
        tari_sidechain::QuorumCertificate {
            header_hash: *qc.header_hash(),
            parent_id: *qc.parent_id().as_hash(),
            signatures: qc
                .signatures()
                .iter()
                .map(|s| {
                    Ok(ValidatorQcSignature {
                        public_key: CompressedPublicKey::from_canonical_bytes(s.public_key.as_bytes()).map_err(
                            |_| {
                                HotStuffError::InvariantError(format!(
                                    "RistrettoPublicKey non-canonical bytes for public key, in \
                                     convert_qc_to_proof_element ({:?})",
                                    s.public_key,
                                ))
                            },
                        )?,
                        signature: convert_validator_block_signature(&s.signature)?,
                    })
                })
                .collect::<Result<_, HotStuffError>>()?,
            decision: qc.decision(),
        },
    ))
}

fn convert_validator_block_signature(
    signature: &SchnorrSignatureBytes,
) -> Result<ValidatorBlockSignature, HotStuffError> {
    let public_nonce =
        CompressedPublicKey::from_canonical_bytes(signature.public_nonce().as_bytes()).map_err(|_| {
            HotStuffError::InvariantError(format!(
                "RistrettoPublicKey non-canonical bytes for public nonce, in convert_validator_block_signature ({:?})",
                signature.public_nonce(),
            ))
        })?;
    let signature = RistrettoSecretKey::from_canonical_bytes(signature.signature().as_bytes()).map_err(|_| {
        HotStuffError::InvariantError(format!(
            "RistrettoPublicKey non-canonical bytes for signature, in convert_validator_block_signature ({:?})",
            signature.signature(),
        ))
    })?;

    Ok(ValidatorBlockSignature::new(public_nonce, signature))
}

#[cfg(test)]
mod tests {
    use tari_common::configuration::Network;
    use tari_common_types::types::FixedHash;
    use tari_crypto::tari_utilities::epoch_time::EpochTime;
    use tari_dan_common_types::{Epoch, ExtraData, NodeHeight, NumPreshards, ShardGroup};
    use tari_sidechain::QuorumDecision;

    use super::*;

    fn seed_hash(seed: u8) -> FixedHash {
        let arr = [seed; 32];
        FixedHash::new(arr)
    }

    #[test]
    fn it_hashes_the_header_identically_to_sidechain_header() {
        let parent_id = seed_hash(1).into_array().into();
        let qc1 = QuorumCertificate::new(
            seed_hash(2),
            parent_id,
            NodeHeight(1),
            Epoch(1),
            ShardGroup::all_shards(NumPreshards::P256),
            vec![],
            QuorumDecision::Accept,
        );

        let network = Network::LocalNet;
        let block = BlockHeader::create(
            network,
            parent_id,
            *qc1.id(),
            NodeHeight(2),
            Epoch(1),
            ShardGroup::all_shards(NumPreshards::P256),
            Default::default(),
            Default::default(),
            &Default::default(),
            1,
            SchnorrSignatureBytes::zero(),
            EpochTime::now().as_u64(),
            FixedHash::zero(),
            ExtraData::new(),
        )
        .unwrap();

        let sidechain_header = SidechainBlockHeader {
            network: network.as_byte(),
            parent_id: *parent_id.as_hash(),
            justify_id: *qc1.id().hash(),
            height: 2,
            epoch: 1,
            shard_group: tari_sidechain::ShardGroup {
                start: 1,
                end_inclusive: 256,
            },
            proposed_by: Default::default(),
            state_merkle_root: Default::default(),
            command_merkle_root: Default::default(),
            signature: ValidatorBlockSignature::new(
                CompressedPublicKey::from_canonical_bytes(block.signature().unwrap().public_nonce().as_bytes())
                    .unwrap(),
                RistrettoSecretKey::from_canonical_bytes(block.signature().unwrap().signature().as_bytes()).unwrap(),
            ),
            metadata_hash: block.calculate_metadata_hash(),
        };

        assert_eq!(sidechain_header.calculate_hash(), block.calculate_hash());
        assert_eq!(sidechain_header.calculate_block_id(), *block.calculate_id().as_hash());
    }
}
