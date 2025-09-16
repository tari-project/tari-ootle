//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use cucumber::when;
use integration_tests::util::cucumber_log;
use tari_template_lib::models::UnclaimedConfidentialOutputAddress;

use crate::TariWorld;

#[when(expr = "I convert commitment in proof {word} into {word} address")]
async fn when_i_convert_commitment_into_address(world: &mut TariWorld, proof_name: String, new_name: String) {
    let proof = world
        .claim_proofs
        .get(&proof_name)
        .unwrap_or_else(|| panic!("BurnProof {} not found", proof_name));
    let address = UnclaimedConfidentialOutputAddress::from_commitment(&proof.claim_proof.commitment);
    cucumber_log(format!(
        "Converted commitment {} into address: {}",
        proof.claim_proof.commitment, address
    ));
    world.substate_ids.insert(new_name, address.into());
}
