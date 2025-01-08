//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_dan_storage::{
    consensus_models::{Block, BlockHeader, QuorumCertificate, QuorumDecision},
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
    ValidatorQcSignature,
};

use crate::hotstuff::HotStuffError;

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::eviction_proof";

pub fn generate_eviction_proofs<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    tip_qc: &QuorumCertificate,
    committed_blocks_with_evictions: &[Block],
) -> Result<Vec<EvictionProof>, HotStuffError> {
    let num_evictions = committed_blocks_with_evictions
        .iter()
        .map(|b| b.all_node_evictions().count())
        .sum();

    let mut proofs = Vec::with_capacity(num_evictions);
    for block in committed_blocks_with_evictions {
        // First generate a commit proof for the block which is shared by all EvictionProofs
        let block_commit_proof = generate_block_commit_proof(tx, tip_qc, block)?;

        for (idx, command) in block.commands().iter().enumerate() {
            let Some(atom) = command.evict_node() else {
                continue;
            };
            info!(target: LOG_TARGET, "🦶 Generating eviction proof for validator: {atom}");
            let inclusion_proof = block.compute_command_inclusion_proof(idx)?;
            let atom = EvictNodeAtom::new(atom.public_key.clone());
            let commit_command_proof = CommandCommitProof::new(atom, block_commit_proof.clone(), inclusion_proof);
            let proof = EvictionProof::new(commit_command_proof);
            proofs.push(proof);
        }
    }

    Ok(proofs)
}

fn generate_block_commit_proof<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    tip_qc: &QuorumCertificate,
    commit_block: &Block,
) -> Result<SidechainBlockCommitProof, HotStuffError> {
    let mut proof_elements = Vec::with_capacity(3);

    if commit_block.is_dummy() || commit_block.signature().is_none() {
        return Err(HotStuffError::InvariantError(format!(
            "Commit block is a dummy block or has no signature in generate_block_commit_proof ({commit_block})",
        )));
    }

    debug!(target: LOG_TARGET, "Add tip_qc: {tip_qc}");
    proof_elements.push(convert_qc_to_proof_element(tip_qc));

    let mut block = tip_qc.get_block(tx)?;
    while block.id() != commit_block.id() {
        if block.justifies_parent() {
            debug!(target: LOG_TARGET, "Add justify: {}", block.justify());
            proof_elements.push(convert_qc_to_proof_element(block.justify()));
            block = block.get_parent(tx)?;
        } else {
            block = block.get_parent(tx)?;
            let mut dummy_chain = vec![ChainLink {
                header_hash: block.header().calculate_hash(),
                parent_id: *block.parent().hash(),
            }];
            debug!(target: LOG_TARGET, "add dummy chain: {block}");
            let parent_id = *block.parent();
            let qc = block.into_justify();
            block = Block::get(tx, &parent_id)?;
            while block.id() != qc.block_id() {
                debug!(target: LOG_TARGET, "add dummy chain: {block} QC: {qc}");
                dummy_chain.push(ChainLink {
                    header_hash: block.header().calculate_hash(),
                    parent_id: *block.parent().hash(),
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
            proof_elements.push(convert_qc_to_proof_element(&qc));
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
        header: convert_block_to_sidechain_block_header(commit_block.header()),
        proof_elements,
    };

    Ok(command_commit_proof)
}

pub fn convert_block_to_sidechain_block_header(header: &BlockHeader) -> SidechainBlockHeader {
    SidechainBlockHeader {
        network: header.network().as_byte(),
        parent_id: *header.parent().hash(),
        justify_id: *header.justify_id().hash(),
        height: header.height().as_u64(),
        epoch: header.epoch().as_u64(),
        shard_group: tari_sidechain::ShardGroup {
            start: header.shard_group().start().as_u32(),
            end_inclusive: header.shard_group().end().as_u32(),
        },
        proposed_by: header.proposed_by().clone(),
        total_leader_fee: header.total_leader_fee(),
        state_merkle_root: *header.state_merkle_root(),
        command_merkle_root: *header.command_merkle_root(),
        is_dummy: header.is_dummy(),
        foreign_indexes_hash: header.create_foreign_indexes_hash(),
        signature: header.signature().expect("checked by caller").clone(),
        timestamp: header.timestamp(),
        base_layer_block_height: header.base_layer_block_height(),
        base_layer_block_hash: *header.base_layer_block_hash(),
        extra_data_hash: header.create_extra_data_hash(),
    }
}

fn convert_qc_to_proof_element(qc: &QuorumCertificate) -> CommitProofElement {
    CommitProofElement::QuorumCertificate(tari_sidechain::QuorumCertificate {
        header_hash: *qc.header_hash(),
        parent_id: *qc.parent_id().hash(),
        signatures: qc
            .signatures()
            .iter()
            .map(|s| ValidatorQcSignature {
                public_key: s.public_key.clone(),
                signature: s.signature.clone(),
            })
            .collect(),
        decision: match qc.decision() {
            QuorumDecision::Accept => tari_sidechain::QuorumDecision::Accept,
            QuorumDecision::Reject => tari_sidechain::QuorumDecision::Reject,
        },
    })
}
