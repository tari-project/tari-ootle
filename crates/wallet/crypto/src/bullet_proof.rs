//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::iter;

use tari_crypto::{
    commitment::ExtensionDegree,
    errors::RangeProofError,
    extended_range_proof::ExtendedRangeProofService,
    ristretto::{
        RistrettoSecretKey,
        bulletproofs_plus::{RistrettoExtendedMask, RistrettoExtendedWitness},
    },
};
use tari_engine_types::crypto::{MAX_LAZY_BP_AGG_FACTORS, get_static_range_proof_service};
use tari_template_lib_types::crypto::RangeProofBytes;

use crate::OutputWitness;

pub fn generate_extended_bullet_proof<'a, I: IntoIterator<Item = &'a OutputWitness>>(
    statements: I,
) -> Result<RangeProofBytes, RangeProofError> {
    let mut extended_witnesses = statements
        .into_iter()
        .map(|stmt| {
            let extended_mask =
                RistrettoExtendedMask::assign(ExtensionDegree::DefaultPedersen, vec![stmt.mask.clone()]).unwrap();
            RistrettoExtendedWitness {
                mask: extended_mask,
                value: stmt.amount,
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

#[cfg(test)]
mod tests {
    use tari_crypto::{keys::SecretKey, ristretto::RistrettoPublicKey, tari_utilities::ByteArray};
    use tari_engine_types::crypto::range_proof::validate_bullet_proof;
    use tari_template_lib_types::{
        EncryptedData,
        crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes},
        stealth::UnspentOutput,
    };

    use super::*;

    fn witness(amount: u64) -> OutputWitness {
        OutputWitness {
            amount,
            mask: RistrettoSecretKey::random(&mut rand::rng()),
            sender_public_nonce: RistrettoPublicKey::default(),
            minimum_value_promise: 0,
            encrypted_data: EncryptedData::empty(),
            resource_view_key: None,
        }
    }

    fn to_output(witness: &OutputWitness) -> UnspentOutput {
        UnspentOutput {
            commitment: PedersenCommitmentBytes::from_bytes(witness.to_commitment().as_bytes()).unwrap(),
            sender_public_nonce: RistrettoPublicKeyBytes::zero(),
            encrypted_data: EncryptedData::empty(),
            minimum_value_promise: witness.minimum_value_promise,
            viewable_balance_proof: None,
        }
    }

    /// Prover and verifier pad independently to `n.next_power_of_two()`, so every count up to the cap must agree on
    /// the aggregation factor and have a service backing it.
    #[test]
    fn proves_and_verifies_at_every_supported_output_count() {
        for n in 1..=MAX_LAZY_BP_AGG_FACTORS {
            let witnesses = (0..n).map(|i| witness(1000 + i as u64)).collect::<Vec<_>>();
            let proof = generate_extended_bullet_proof(&witnesses).unwrap();
            let outputs = witnesses.iter().map(to_output).collect::<Vec<_>>();
            validate_bullet_proof(&proof, outputs.iter()).unwrap_or_else(|e| panic!("n={n}: {e}"));
        }
    }

    #[test]
    fn rejects_more_outputs_than_the_cap() {
        let witnesses = (0..=MAX_LAZY_BP_AGG_FACTORS)
            .map(|i| witness(1000 + i as u64))
            .collect::<Vec<_>>();
        generate_extended_bullet_proof(&witnesses).unwrap_err();
    }
}
