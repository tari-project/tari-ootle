//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeSet;

use ootle_byte_type::ConvertFromByteType;
use tari_crypto::{
    extended_range_proof::Statement,
    ristretto::{RistrettoPublicKey, pedersen::PedersenCommitment},
};
use tari_template_lib::types::{
    crypto::UtxoTag,
    stealth::{SpendAuthorization, StealthOutputsStatement},
};

use crate::{
    UtxoOutput,
    crypto::{
        MAX_LAZY_BP_AGG_FACTORS,
        ValidateOutputBody,
        range_proof::validate_bullet_proof_statements,
        validate_elgamal_verifiable_balance_proof,
    },
    resource_container::ResourceError,
};

#[derive(Debug, Clone)]
pub struct ValidatedStealthOutput {
    pub output: ValidateOutputBody,
    pub auth: SpendAuthorization,
    pub tag: UtxoTag,
}

impl ValidatedStealthOutput {
    pub fn into_utxo_output(self) -> UtxoOutput {
        UtxoOutput {
            auth: self.auth,
            output: self.output.into_output_body(),
            tag: self.tag,
        }
    }
}

pub fn validate_stealth_outputs_statement(
    stmt: &StealthOutputsStatement,
    view_key: Option<&RistrettoPublicKey>,
) -> Result<Vec<ValidatedStealthOutput>, ResourceError> {
    if stmt.outputs.len() > MAX_LAZY_BP_AGG_FACTORS {
        return Err(ResourceError::InvalidConfidentialProof {
            details: format!(
                "Range proof aggregation factor {} exceeds the maximum supported {}",
                stmt.outputs.len(),
                MAX_LAZY_BP_AGG_FACTORS
            ),
        });
    }

    if stmt.outputs.is_empty() {
        // Edge case: Asserts that the bulletproof is 0 bytes if there are no outputs
        validate_bullet_proof_statements(&stmt.agg_range_proof, Vec::new())?;
        return Ok(vec![]);
    }

    let mut seen_commitments = BTreeSet::new();
    let mut decompressed = Vec::with_capacity(stmt.outputs.len());
    for statement in &stmt.outputs {
        let output = &statement.output;
        if !seen_commitments.insert(output.commitment) {
            return Err(ResourceError::InvalidConfidentialProof {
                details: format!(
                    "Duplicate output commitment {} in stealth outputs statement",
                    output.commitment
                ),
            });
        }

        let output_commitment = PedersenCommitment::convert_from_byte_type(&output.commitment).map_err(|_| {
            ResourceError::InvalidConfidentialProof {
                details: "Invalid commitment".to_string(),
            }
        })?;

        let output_public_nonce =
            RistrettoPublicKey::convert_from_byte_type(&output.sender_public_nonce).map_err(|_| {
                ResourceError::InvalidConfidentialProof {
                    details: "Invalid sender public nonce".to_string(),
                }
            })?;

        // Unspendable `{no key, no conditions}` outputs are unrepresentable because `SpendAuthorization` has no such
        // variant, so every output has at least one spend path. When a `spend_key` is present, it reaches the auth
        // scope as a badge compared bytewise without decompression and must be satisfiable by an `sr25519` transaction
        // signer, which rejects both non-canonical encodings and the identity key (`[0u8; 32]`).
        if let Some(spend_key) = statement.auth.spend_key() &&
            (spend_key.is_zero() || RistrettoPublicKey::convert_from_byte_type(spend_key).is_err())
        {
            return Err(ResourceError::InvalidSpend {
                details: format!("Stealth output spend key is not a valid signer public key: {spend_key}"),
            });
        }

        match (view_key.is_some(), output.viewable_balance_proof.is_some()) {
            (false, true) => {
                return Err(ResourceError::InvalidConfidentialProof {
                    details: "ViewableBalanceProof provided for a resource that is not viewable".to_string(),
                });
            },
            (true, false) => {
                return Err(ResourceError::InvalidConfidentialProof {
                    details: "ViewableBalanceProof is required for a viewable resource".to_string(),
                });
            },
            _ => {},
        }

        decompressed.push((output_commitment, output_public_nonce));
    }

    if stmt.agg_range_proof.is_empty() {
        return Err(ResourceError::InvalidRangeProof {
            details: "Range proof is empty".to_string(),
        });
    }

    let mut outputs = Vec::with_capacity(stmt.outputs.len());
    let mut bp_statements = Vec::with_capacity(stmt.outputs.len());
    for (statement, (output_commitment, output_public_nonce)) in stmt.outputs.iter().zip(decompressed) {
        let output = &statement.output;
        let viewable_balance = validate_elgamal_verifiable_balance_proof(
            &output_commitment,
            view_key,
            output.viewable_balance_proof.as_ref(),
        )?;
        bp_statements.push(Statement {
            commitment: output_commitment.clone(),
            minimum_value_promise: output.minimum_value_promise,
        });
        outputs.push(ValidatedStealthOutput {
            output: ValidateOutputBody {
                commitment: output_commitment,
                public_nonce: output_public_nonce,
                encrypted_data: output.encrypted_data.clone(),
                minimum_value_promise: output.minimum_value_promise,
                viewable_balance,
            },
            auth: statement.auth.clone(),
            tag: statement.tag,
        });
    }

    validate_bullet_proof_statements(&stmt.agg_range_proof, bp_statements)?;

    Ok(outputs)
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{
        commitment::{ExtensionDegree, HomomorphicCommitmentFactory},
        extended_range_proof::ExtendedRangeProofService,
        keys::{PublicKey, SecretKey},
        ristretto::{
            RistrettoSecretKey,
            bulletproofs_plus::{RistrettoExtendedMask, RistrettoExtendedWitness},
        },
        tari_utilities::ByteArray,
    };
    use tari_template_lib::types::{
        EncryptedData,
        Hash32,
        crypto::{PedersenCommitmentBytes, RangeProofBytes, RistrettoPublicKeyBytes, Scalar32Bytes},
        stealth::{StealthUnspentOutput, UnspentOutput, ViewableBalanceProof},
    };

    use super::*;
    use crate::crypto::{commit_u64_amount, get_commitment_factory, get_static_range_proof_service, messages};

    fn sample_stealth_output(mask_seed: u64, value: u64) -> StealthUnspentOutput {
        let mask = RistrettoSecretKey::from(mask_seed);
        let commitment = commit_u64_amount(&mask, value).to_byte_type();
        let pub_key = RistrettoPublicKey::from_secret_key(&mask).to_byte_type();
        StealthUnspentOutput {
            output: UnspentOutput {
                commitment,
                sender_public_nonce: pub_key,
                encrypted_data: EncryptedData::try_from(vec![0u8; EncryptedData::min_size()]).unwrap(),
                minimum_value_promise: 1,
                viewable_balance_proof: None,
            },
            auth: SpendAuthorization::Key(pub_key),
            tag: UtxoTag::new(7),
        }
    }

    fn single_output_range_proof(mask_seed: u64, value: u64) -> RangeProofBytes {
        let mask = RistrettoSecretKey::from(mask_seed);
        let witness = RistrettoExtendedWitness {
            mask: RistrettoExtendedMask::assign(ExtensionDegree::DefaultPedersen, vec![mask]).unwrap(),
            value,
            minimum_value_promise: 1,
        };
        let bytes = get_static_range_proof_service(1)
            .construct_extended_proof(vec![witness], None)
            .unwrap();
        RangeProofBytes::try_from(bytes).unwrap()
    }

    fn sample_viewable_balance_proof(
        mask_seed: u64,
        value: u64,
        view_key: &RistrettoPublicKey,
    ) -> ViewableBalanceProof {
        let mask = RistrettoSecretKey::from(mask_seed);
        let val_scalar = RistrettoSecretKey::from(value);
        let commitment = commit_u64_amount(&mask, value);

        let r = RistrettoSecretKey::from(mask_seed + 100);
        let k_m = RistrettoSecretKey::from(mask_seed + 200);
        let k_v = RistrettoSecretKey::from(mask_seed + 300);
        let k_r = RistrettoSecretKey::from(mask_seed + 400);

        let elgamal_public_nonce = RistrettoPublicKey::from_secret_key(&r);
        let elgamal_encrypted = RistrettoPublicKey::from_secret_key(&val_scalar) + &r * view_key;
        let c_prime = get_commitment_factory().commit(&k_m, &k_v);
        let e_prime = RistrettoPublicKey::from_secret_key(&k_v) + &k_r * view_key;
        let r_prime = RistrettoPublicKey::from_secret_key(&k_r);

        let zero_scalar = Scalar32Bytes::from([0u8; 32]);
        let mut proof = ViewableBalanceProof {
            elgamal_encrypted: elgamal_encrypted.to_byte_type(),
            elgamal_public_nonce: elgamal_public_nonce.to_byte_type(),
            c_prime: c_prime.to_byte_type(),
            e_prime: e_prime.to_byte_type(),
            r_prime: r_prime.to_byte_type(),
            s_v: zero_scalar,
            s_m: zero_scalar,
            s_r: zero_scalar,
        };

        let e = RistrettoSecretKey::from_uniform_bytes(&messages::viewable_balance_proof64(
            &commitment,
            view_key,
            proof.as_challenge_fields(),
        ))
        .unwrap();

        proof.s_v = Scalar32Bytes::from_bytes((&e * &val_scalar + k_v).as_bytes()).unwrap();
        proof.s_m = Scalar32Bytes::from_bytes((&e * &mask + k_m).as_bytes()).unwrap();
        proof.s_r = Scalar32Bytes::from_bytes((&e * &r + k_r).as_bytes()).unwrap();
        proof
    }

    #[test]
    fn accepts_valid_key_script_and_key_and_script_outputs_and_rejects_bad_range_proof() {
        let range_proof = single_output_range_proof(7, 100);
        let valid_pk = RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(77u64)).to_byte_type();
        let condition_root = Hash32::from_array([5u8; 32]);

        for auth in [
            SpendAuthorization::Key(valid_pk),
            SpendAuthorization::Script(condition_root),
            SpendAuthorization::KeyAndScript {
                spend_key: valid_pk,
                condition_root,
            },
        ] {
            let mut out = sample_stealth_output(7, 100);
            out.auth = auth.clone();
            let expected_nonce = out.output.sender_public_nonce;
            let stmt = StealthOutputsStatement {
                outputs: vec![out],
                revealed_output: None,
                agg_range_proof: range_proof.clone(),
            };

            let validated = validate_stealth_outputs_statement(&stmt, None).unwrap();
            assert_eq!(validated.len(), 1);
            assert_eq!(validated[0].auth, auth);
            assert_eq!(validated[0].tag, UtxoTag::new(7));
            assert_eq!(validated[0].output.minimum_value_promise, 1);
            assert!(validated[0].output.viewable_balance.is_none());

            let utxo = validated.into_iter().next().unwrap().into_utxo_output();
            assert_eq!(utxo.auth, auth);
            assert_eq!(utxo.tag, UtxoTag::new(7));
            assert_eq!(utxo.output.public_nonce, expected_nonce);
        }

        // Non-empty invalid range proof on an otherwise valid output must fail Bulletproofs+ verification.
        let invalid_proof_stmt = StealthOutputsStatement {
            outputs: vec![sample_stealth_output(7, 100)],
            revealed_output: None,
            agg_range_proof: RangeProofBytes::try_from(vec![0xabu8; 64]).unwrap(),
        };
        let err = validate_stealth_outputs_statement(&invalid_proof_stmt, None).unwrap_err();
        assert!(
            matches!(&err, ResourceError::InvalidRangeProof { .. }),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn rejects_non_canonical_and_identity_spend_keys_in_key_and_key_and_script_outputs() {
        let non_canonical = RistrettoPublicKeyBytes::from_bytes(&[0xff; 32]).unwrap();
        let identity = RistrettoPublicKeyBytes::zero();
        assert!(RistrettoPublicKey::convert_from_byte_type(&non_canonical).is_err());
        assert!(RistrettoPublicKey::convert_from_byte_type(&identity).is_ok());

        let range_proof = single_output_range_proof(7, 100);
        for invalid_key in [non_canonical, identity] {
            for auth in [
                SpendAuthorization::Key(invalid_key),
                SpendAuthorization::KeyAndScript {
                    spend_key: invalid_key,
                    condition_root: Hash32::zero(),
                },
            ] {
                let mut out = sample_stealth_output(7, 100);
                out.auth = auth;
                let stmt = StealthOutputsStatement {
                    outputs: vec![out],
                    revealed_output: None,
                    agg_range_proof: range_proof.clone(),
                };

                let err = validate_stealth_outputs_statement(&stmt, None).unwrap_err();
                assert!(
                    matches!(
                        &err,
                        ResourceError::InvalidSpend { details }
                            if details.contains("not a valid signer public key")
                    ),
                    "unexpected error: {err:?}"
                );
            }
        }
    }

    #[test]
    fn rejects_duplicate_output_commitments_in_stealth_outputs_statement() {
        let dup = sample_stealth_output(9, 50);
        let stmt = StealthOutputsStatement {
            outputs: vec![dup.clone(), dup],
            revealed_output: None,
            agg_range_proof: RangeProofBytes::empty(),
        };

        let err = validate_stealth_outputs_statement(&stmt, None).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidConfidentialProof { details }
                    if details.contains("Duplicate output commitment")
            ),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn validates_viewable_balance_proofs_and_fast_fails_empty_range_proof() {
        let view_sk = RistrettoSecretKey::from(42u64);
        let view_pk = RistrettoPublicKey::from_secret_key(&view_sk);
        let vb_proof = sample_viewable_balance_proof(7, 100, &view_pk);
        let range_proof = single_output_range_proof(7, 100);

        // 1. Missing ViewableBalanceProof when view_key is Some fails before range proof check.
        let missing_vb = StealthOutputsStatement {
            outputs: vec![sample_stealth_output(7, 100)],
            revealed_output: None,
            agg_range_proof: RangeProofBytes::empty(),
        };
        let err = validate_stealth_outputs_statement(&missing_vb, Some(&view_pk)).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidConfidentialProof { details }
                    if details.contains("ViewableBalanceProof is required")
            ),
            "unexpected error: {err:?}"
        );

        // 2. Unexpected ViewableBalanceProof when view_key is None fails before range proof check.
        let mut unexpected_out = sample_stealth_output(7, 100);
        unexpected_out.output.viewable_balance_proof = Some(vb_proof.clone());
        let unexpected_vb = StealthOutputsStatement {
            outputs: vec![unexpected_out],
            revealed_output: None,
            agg_range_proof: RangeProofBytes::empty(),
        };
        let err = validate_stealth_outputs_statement(&unexpected_vb, None).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidConfidentialProof { details }
                    if details.contains("not viewable")
            ),
            "unexpected error: {err:?}"
        );

        // 3. Empty range proof with tampered DLEQ scalars fails fast on `Range proof is empty` BEFORE running ElGamal
        //    scalar multiplications.
        let mut bad_dleq = vb_proof.clone();
        bad_dleq.s_v = Scalar32Bytes::from_bytes(RistrettoSecretKey::from(999u64).as_bytes()).unwrap();
        let mut empty_rp_out = sample_stealth_output(7, 100);
        empty_rp_out.output.viewable_balance_proof = Some(bad_dleq.clone());
        let empty_rp_stmt = StealthOutputsStatement {
            outputs: vec![empty_rp_out],
            revealed_output: None,
            agg_range_proof: RangeProofBytes::empty(),
        };
        let err = validate_stealth_outputs_statement(&empty_rp_stmt, Some(&view_pk)).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidRangeProof { details }
                    if details == "Range proof is empty"
            ),
            "unexpected error: {err:?}"
        );

        // 4. Non-empty range proof + tampered DLEQ proof fails ElGamal verification.
        let mut tampered_vb_out = sample_stealth_output(7, 100);
        tampered_vb_out.output.viewable_balance_proof = Some(bad_dleq);
        let tampered_vb_stmt = StealthOutputsStatement {
            outputs: vec![tampered_vb_out],
            revealed_output: None,
            agg_range_proof: range_proof.clone(),
        };
        let err = validate_stealth_outputs_statement(&tampered_vb_stmt, Some(&view_pk)).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidConfidentialProof { details }
                    if details.contains("Invalid viewable balance proof")
            ),
            "unexpected error: {err:?}"
        );

        // 5. Valid ViewableBalanceProof + valid range proof succeeds and populates `viewable_balance`.
        let mut valid_vb_out = sample_stealth_output(7, 100);
        valid_vb_out.output.viewable_balance_proof = Some(vb_proof);
        let valid_vb_stmt = StealthOutputsStatement {
            outputs: vec![valid_vb_out],
            revealed_output: None,
            agg_range_proof: range_proof,
        };
        let validated = validate_stealth_outputs_statement(&valid_vb_stmt, Some(&view_pk)).unwrap();
        assert_eq!(validated.len(), 1);
        assert!(validated[0].output.viewable_balance.is_some());
    }

    #[test]
    fn rejects_malformed_output_keys_before_range_proof_verification() {
        let non_canonical = RistrettoPublicKeyBytes::from_bytes(&[0xff; 32]).unwrap();

        let mut bad_nonce = sample_stealth_output(11, 25);
        bad_nonce.output.sender_public_nonce = non_canonical;
        let stmt = StealthOutputsStatement {
            outputs: vec![bad_nonce],
            revealed_output: None,
            // An empty range proof would fail with `InvalidRangeProof` if range-proof checks ran first.
            agg_range_proof: RangeProofBytes::empty(),
        };
        let err = validate_stealth_outputs_statement(&stmt, None).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidConfidentialProof { details }
                    if details.contains("Invalid sender public nonce")
            ),
            "unexpected error: {err:?}"
        );

        let mut bad_commitment = sample_stealth_output(11, 25);
        bad_commitment.output.commitment = PedersenCommitmentBytes::from_bytes(&[0xff; 32]).unwrap();
        let stmt = StealthOutputsStatement {
            outputs: vec![bad_commitment],
            revealed_output: None,
            agg_range_proof: RangeProofBytes::empty(),
        };
        let err = validate_stealth_outputs_statement(&stmt, None).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidConfidentialProof { details }
                    if details == "Invalid commitment"
            ),
            "unexpected error: {err:?}"
        );

        let mut bad_spend_key = sample_stealth_output(11, 25);
        bad_spend_key.auth = SpendAuthorization::Key(non_canonical);
        let stmt = StealthOutputsStatement {
            outputs: vec![bad_spend_key],
            revealed_output: None,
            agg_range_proof: RangeProofBytes::empty(),
        };
        let err = validate_stealth_outputs_statement(&stmt, None).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidSpend { details }
                    if details.contains("not a valid signer public key")
            ),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn handles_empty_boundary_and_oversized_stealth_output_batches() {
        let empty_ok = StealthOutputsStatement {
            outputs: vec![],
            revealed_output: None,
            agg_range_proof: RangeProofBytes::empty(),
        };
        assert!(validate_stealth_outputs_statement(&empty_ok, None).unwrap().is_empty());

        let empty_with_proof = StealthOutputsStatement {
            outputs: vec![],
            revealed_output: None,
            agg_range_proof: RangeProofBytes::try_from(vec![1u8]).unwrap(),
        };
        let err = validate_stealth_outputs_statement(&empty_with_proof, None).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidConfidentialProof { details }
                    if details.contains("contained no outputs")
            ),
            "unexpected error: {err:?}"
        );

        // Exact `MAX_LAZY_BP_AGG_FACTORS` (16) passes the upper-bound guard and reaches range-proof verification.
        let at_cap = (1..=(MAX_LAZY_BP_AGG_FACTORS as u64))
            .map(|seed| sample_stealth_output(seed, 10))
            .collect();
        let at_cap_stmt = StealthOutputsStatement {
            outputs: at_cap,
            revealed_output: None,
            agg_range_proof: RangeProofBytes::empty(),
        };
        let err = validate_stealth_outputs_statement(&at_cap_stmt, None).unwrap_err();
        assert!(
            matches!(&err, ResourceError::InvalidRangeProof { .. }),
            "unexpected error at exact cap: {err:?}"
        );

        let non_canonical = RistrettoPublicKeyBytes::from_bytes(&[0xff; 32]).unwrap();
        let oversized = (1..=(MAX_LAZY_BP_AGG_FACTORS as u64 + 1))
            .map(|seed| {
                let mut out = sample_stealth_output(seed, 10);
                out.auth = SpendAuthorization::Key(non_canonical);
                out
            })
            .collect();
        let stmt = StealthOutputsStatement {
            outputs: oversized,
            revealed_output: None,
            agg_range_proof: RangeProofBytes::empty(),
        };
        let err = validate_stealth_outputs_statement(&stmt, None).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidConfidentialProof { details }
                    if details.contains("exceeds the maximum supported")
            ),
            "unexpected error: {err:?}"
        );
    }
}
