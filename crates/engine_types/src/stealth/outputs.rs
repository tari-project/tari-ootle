//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeSet;

use ootle_byte_type::ConvertFromByteType;
use tari_crypto::ristretto::{RistrettoPublicKey, pedersen::PedersenCommitment};
use tari_template_lib::types::{
    crypto::UtxoTag,
    stealth::{SpendAuthorization, StealthOutputsStatement},
};

use crate::{
    UtxoOutput,
    crypto::{ValidateOutputBody, range_proof::validate_bullet_proof, validate_elgamal_verifiable_balance_proof},
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
    let mut seen_commitments = BTreeSet::new();
    for statement in &stmt.outputs {
        if !seen_commitments.insert(statement.output.commitment) {
            return Err(ResourceError::InvalidConfidentialProof {
                details: format!(
                    "Duplicate output commitment {} in stealth outputs statement",
                    statement.output.commitment
                ),
            });
        }
    }

    // Edge case: Asserts that the bulletproof is 0 bytes if there are no outputs
    validate_bullet_proof(&stmt.agg_range_proof, stmt.outputs.iter().map(|o| &o.output))?;
    if stmt.outputs.is_empty() {
        return Ok(vec![]);
    }

    let outputs =
        stmt.outputs
            .iter()
            .map(|statement| {
                let output = &statement.output;
                let output_commitment =
                    PedersenCommitment::convert_from_byte_type(&output.commitment).map_err(|_| {
                        ResourceError::InvalidConfidentialProof {
                            details: "Invalid commitment".to_string(),
                        }
                    })?;

                let output_public_nonce = RistrettoPublicKey::convert_from_byte_type(&output.sender_public_nonce)
                    .map_err(|_| ResourceError::InvalidConfidentialProof {
                        details: "Invalid sender public nonce".to_string(),
                    })?;

                let viewable_balance = validate_elgamal_verifiable_balance_proof(
                    &output_commitment,
                    view_key,
                    output.viewable_balance_proof.as_ref(),
                )?;
                let output = ValidateOutputBody {
                    commitment: output_commitment,
                    public_nonce: output_public_nonce,
                    encrypted_data: output.encrypted_data.clone(),
                    minimum_value_promise: output.minimum_value_promise,
                    viewable_balance,
                };

                // Unspendable `{no key, no conditions}` outputs are unrepresentable: `SpendAuthorization` has no such
                // variant, so a decoded output is always spendable by construction — no runtime check needed.
                Ok(ValidatedStealthOutput {
                    output,
                    auth: statement.auth.clone(),
                    tag: statement.tag,
                })
            })
            .collect::<Result<_, ResourceError>>()?;

    Ok(outputs)
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{keys::PublicKey, ristretto::RistrettoSecretKey};
    use tari_template_lib::types::{
        EncryptedData,
        crypto::RangeProofBytes,
        stealth::{StealthUnspentOutput, UnspentOutput},
    };

    use super::*;
    use crate::crypto::commit_u64_amount;

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
            tag: UtxoTag::new(0),
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
}
