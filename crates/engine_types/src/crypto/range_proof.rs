//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::iter;

use tari_crypto::{
    extended_range_proof::{ExtendedRangeProofService, Statement},
    ristretto::{bulletproofs_plus::RistrettoAggregatedPublicStatement, pedersen::PedersenCommitment},
    tari_utilities::ByteArray,
};
use tari_template_lib::{models::UnspentOutput, types::crypto::RangeProofBytes};

use crate::{crypto::bullet_proof_service_factory, resource_container::ResourceError};

pub fn validate_bullet_proof<'a, I: IntoIterator<Item = &'a UnspentOutput>>(
    range_proof: &RangeProofBytes,
    outputs: I,
) -> Result<(), ResourceError> {
    let mut statements = outputs
        .into_iter()
        .map(|stmt| {
            let commitment = PedersenCommitment::from_canonical_bytes(&*stmt.commitment).map_err(|_| {
                ResourceError::InvalidConfidentialProof {
                    details: "Invalid commitment".to_string(),
                }
            })?;
            Ok(Statement {
                commitment,
                minimum_value_promise: stmt.minimum_value_promise,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let agg_factor = statements.len();
    if agg_factor == 0 {
        // No outputs, so no rangeproof needed (revealed mint)
        if range_proof.is_empty() {
            return Ok(());
        }
        return Err(ResourceError::InvalidConfidentialProof {
            details: "Range proof is invalid because it was provided but the proof contained no outputs".to_string(),
        });
    }
    if !agg_factor.is_power_of_two() {
        let num_to_add = agg_factor.next_power_of_two() - agg_factor;
        // If the number of statements is not a power of two, we pad with zero statements
        let default_commitment = PedersenCommitment::default();
        statements.extend(iter::repeat_n(
            Statement {
                commitment: default_commitment,
                minimum_value_promise: 0,
            },
            num_to_add,
        ));
    }

    let public_statement = RistrettoAggregatedPublicStatement::init(statements).unwrap();

    let proofs = vec![range_proof.as_ref()];
    bullet_proof_service_factory(agg_factor)
        .verify_batch(proofs, vec![&public_statement])
        .map_err(|e| ResourceError::InvalidRangeProof { details: e.to_string() })?;

    Ok(())
}
