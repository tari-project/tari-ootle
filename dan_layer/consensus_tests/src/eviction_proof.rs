//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::CompressedPublicKey;
use tari_consensus::hotstuff::commit_proofs::convert_block_to_sidechain_block_header;
use tari_crypto::tari_utilities::ByteArray;
use tari_dan_storage::consensus_models::Block;

use crate::support::load_json_fixture;

#[test]
fn it_produces_a_summarized_header_that_hashes_to_the_original() {
    let block = load_json_fixture::<Block>("block.json");
    let sidechain_block = convert_block_to_sidechain_block_header(block.header()).unwrap();
    assert_eq!(sidechain_block.metadata_hash, block.header().calculate_metadata_hash());
    assert_eq!(
        sidechain_block.signature.get_compressed_public_nonce().as_bytes(),
        block
            .header()
            .signature()
            .expect("checked by caller")
            .public_nonce()
            .as_bytes()
    );
    assert_eq!(
        sidechain_block.signature.get_signature().as_bytes(),
        block
            .header()
            .signature()
            .expect("checked by caller")
            .signature()
            .as_bytes()
    );
    assert_eq!(
        sidechain_block.command_merkle_root,
        *block.header().command_merkle_root()
    );
    assert_eq!(sidechain_block.state_merkle_root, *block.header().state_merkle_root());
    assert_eq!(
        sidechain_block.proposed_by,
        CompressedPublicKey::from_canonical_bytes(block.header().proposed_by().as_bytes()).unwrap()
    );
    assert_eq!(
        sidechain_block.shard_group.start,
        block.header().shard_group().start().as_u32()
    );
    assert_eq!(
        sidechain_block.shard_group.end_inclusive,
        block.header().shard_group().end().as_u32()
    );
    assert_eq!(sidechain_block.epoch, block.header().epoch().as_u64());
    assert_eq!(sidechain_block.height, block.header().height().as_u64());
    assert_eq!(sidechain_block.justify_id, *block.header().justify_id().hash());
    assert_eq!(sidechain_block.parent_id, *block.header().parent().hash());
    assert_eq!(sidechain_block.network, block.header().network().as_byte());

    // Finally check the hash matches
    assert_eq!(sidechain_block.calculate_hash(), block.header().calculate_hash());
    assert_eq!(
        sidechain_block.calculate_block_id(),
        *block.header().calculate_id().hash()
    );
}
