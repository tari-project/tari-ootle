//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::{
    extended_range_proof::{ExtendedRangeProofService, Statement},
    ristretto::{bulletproofs_plus::RistrettoAggregatedPublicStatement, pedersen::PedersenCommitment},
    tari_utilities::ByteArray,
};
use tari_template_lib::{models::ConfidentialStatement, types::crypto::RangeProofBytes};

use crate::{crypto::get_range_proof_service, resource_container::ResourceError};

pub fn validate_bullet_proof<'a, I: IntoIterator<Item = &'a ConfidentialStatement>>(
    range_proof: &RangeProofBytes,
    outputs: I,
) -> Result<(), ResourceError> {
    let statements = outputs
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

    // Either 0, 1 or 2
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

    let public_statement = RistrettoAggregatedPublicStatement::init(statements).unwrap();

    let proofs = vec![range_proof.as_ref()];
    get_range_proof_service(agg_factor)
        .verify_batch(proofs, vec![&public_statement])
        .map_err(|e| ResourceError::InvalidConfidentialProof {
            details: format!("Invalid range proof: {}", e),
        })?;

    Ok(())
}
