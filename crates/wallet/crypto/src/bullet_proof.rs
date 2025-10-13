//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::iter;

use tari_crypto::{
    commitment::ExtensionDegree,
    errors::RangeProofError,
    extended_range_proof::ExtendedRangeProofService,
    ristretto::{
        bulletproofs_plus::{RistrettoExtendedMask, RistrettoExtendedWitness},
        RistrettoSecretKey,
    },
};
use tari_engine_types::crypto::{get_static_range_proof_service, MAX_LAZY_BP_AGG_FACTORS};
use tari_template_lib::types::crypto::RangeProofBytes;

use crate::UnblindedOutputWitness;

pub fn generate_extended_bullet_proof<'a, I: IntoIterator<Item = &'a UnblindedOutputWitness>>(
    statements: I,
) -> Result<RangeProofBytes, RangeProofError> {
    let mut extended_witnesses = statements
        .into_iter()
        .map(|stmt| {
            let extended_mask =
                RistrettoExtendedMask::assign(ExtensionDegree::DefaultPedersen, vec![stmt.mask.clone()]).unwrap();
            RistrettoExtendedWitness {
                mask: extended_mask,
                value: stmt
                    .amount
                    .to_u64_checked()
                    .expect("BUG: Invalid output statement amount provided to generate_extended_bullet_proof"),
                minimum_value_promise: stmt.minimum_value_promise,
            }
        })
        .collect::<Vec<_>>();
    if extended_witnesses.is_empty() {
        // If no output statements are provided, we return an empty range proof
        return Ok(RangeProofBytes::empty());
    }
    if !extended_witnesses.len().is_power_of_two() {
        let num_to_add = extended_witnesses.len().next_power_of_two() - extended_witnesses.len();
        let default_mask =
            RistrettoExtendedMask::assign(ExtensionDegree::DefaultPedersen, vec![RistrettoSecretKey::default()])
                .unwrap();
        // If the number of statements is not a power of two, we pad with zero witnesses
        extended_witnesses.extend(iter::repeat_n(
            RistrettoExtendedWitness {
                mask: default_mask,
                value: 0,
                minimum_value_promise: 0,
            },
            num_to_add,
        ));
    }

    let agg_factor = extended_witnesses.len();
    if agg_factor > MAX_LAZY_BP_AGG_FACTORS {
        return Err(RangeProofError::ProofConstructionError {
            reason: format!(
                "Range proof aggregation factor {} exceeds the maximum supported {}",
                agg_factor, MAX_LAZY_BP_AGG_FACTORS
            ),
        });
    }

    let output_range_proof =
        get_static_range_proof_service(agg_factor).construct_extended_proof(extended_witnesses, None)?;

    RangeProofBytes::try_from(output_range_proof)
        .map_err(|e| RangeProofError::ProofConstructionError { reason: e.to_string() })
}
