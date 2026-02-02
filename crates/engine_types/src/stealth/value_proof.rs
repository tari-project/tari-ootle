//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::FromByteType;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
};
use tari_template_lib::{
    prelude::{
        crypto::{StealthValueProof, ValueKnowledgeProof},
        Amount,
        PedersenCommitmentBytes,
    },
    types::UtxoId,
};

use crate::{
    crypto::{commit_amount_checked, convert_amount_to_secret, messages, ElgamalVerifiableBalanceBytes},
    resource_container::ResourceError,
};

pub fn validate_value_proof(
    commitment_bytes: &PedersenCommitmentBytes,
    elgamal_verifiable_balance: Option<&ElgamalVerifiableBalanceBytes>,
    proof: &StealthValueProof,
) -> Result<Amount, ResourceError> {
    if proof.value.is_negative() {
        return Err(ResourceError::UtxoBurnFailed {
            id: UtxoId::from(*commitment_bytes),
            details: "Value proof amount cannot be negative".to_string(),
        });
    }

    let commitment: PedersenCommitment =
        commitment_bytes
            .try_from_byte_type()
            .map_err(|e| ResourceError::UtxoBurnFailed {
                id: UtxoId::from(*commitment_bytes),
                details: format!("Invalid commitment bytes: {}", e),
            })?;

    match proof.knowledge_proof {
        ValueKnowledgeProof::Commitment { mask_knowledge_proof } => {
            let Some(commit_amount) = commit_amount_checked(&RistrettoSecretKey::default(), proof.value) else {
                return Err(ResourceError::UtxoBurnFailed {
                    id: UtxoId::from(*commitment_bytes),
                    details: "Value proof amount is too large".to_string(),
                });
            };
            let public_mask = commitment.as_public_key() - commit_amount.as_public_key();

            let sig: RistrettoSchnorr =
                mask_knowledge_proof
                    .try_from_byte_type()
                    .map_err(|e| ResourceError::UtxoBurnFailed {
                        id: UtxoId::from(*commitment_bytes),
                        details: format!("Invalid mask knowledge proof bytes: {}", e),
                    })?;

            let message = messages::value_proof_message(commitment_bytes, &proof.value);

            if !sig.verify(&public_mask, message) {
                return Err(ResourceError::UtxoBurnFailed {
                    id: UtxoId::from(*commitment_bytes),
                    details: "Invalid mask knowledge proof".to_string(),
                });
            }
        },
        ValueKnowledgeProof::ElgamalEncrypted { reveal_key } => {
            let elgamal = elgamal_verifiable_balance.ok_or_else(|| ResourceError::UtxoBurnFailed {
                id: UtxoId::from(*commitment_bytes),
                details: "Utxo does not have a viewable balance".to_string(),
            })?;

            let encrypted: RistrettoPublicKey =
                elgamal
                    .encrypted
                    .try_from_byte_type()
                    .map_err(|e| ResourceError::UtxoBurnFailed {
                        id: UtxoId::from(*commitment_bytes),
                        details: format!("Invalid encrypted balance bytes: {}", e),
                    })?;

            let reveal_key: RistrettoPublicKey =
                reveal_key
                    .try_from_byte_type()
                    .map_err(|e| ResourceError::UtxoBurnFailed {
                        id: UtxoId::from(*commitment_bytes),
                        details: format!("Invalid reveal key bytes: {}", e),
                    })?;

            // E - R.p = v.G
            let check_value = encrypted - reveal_key;

            let value = convert_amount_to_secret(&proof.value);
            let value_g = RistrettoPublicKey::from_secret_key(&value);
            if value_g != check_value {
                return Err(ResourceError::UtxoBurnFailed {
                    id: UtxoId::from(*commitment_bytes),
                    details: "Invalid Elgamal encrypted value proof".to_string(),
                });
            }
        },
    }

    Ok(proof.value)
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use rand::rngs::OsRng;
    use tari_crypto::keys::SecretKey;
    use tari_template_lib::types::crypto::{StealthValueProof, ValueKnowledgeProof};

    use super::*;

    #[test]
    fn it_proves_knowledge_of_the_value() {
        let mask = RistrettoSecretKey::random(&mut OsRng);
        let value = 100_321_123u128.into();
        let commitment = commit_amount_checked(&mask, value).unwrap();
        let commitment_bytes = commitment.to_byte_type();

        // Create the proof of knowledge of the value
        let message = messages::value_proof_message(&commitment_bytes, &value);
        let sig = RistrettoSchnorr::sign(&mask, message, &mut OsRng).unwrap();

        let proof = StealthValueProof {
            value,
            knowledge_proof: ValueKnowledgeProof::Commitment {
                mask_knowledge_proof: sig.to_byte_type(),
            },
        };

        // Validate the proof
        let amount = validate_value_proof(&commitment_bytes, None, &proof).unwrap();
        assert_eq!(amount, value);
    }

    #[test]
    fn it_fails_if_the_value_differs() {
        let mask = RistrettoSecretKey::random(&mut OsRng);
        let value = 100_321_123u128.into();
        let commitment = commit_amount_checked(&mask, value).unwrap();
        let commitment_bytes = commitment.to_byte_type();

        let other_value = value + Amount::ONE;

        // Create the proof of knowledge of the value
        let message = messages::value_proof_message(&commitment_bytes, &other_value);
        let sig = RistrettoSchnorr::sign(&mask, message, &mut OsRng).unwrap();

        let proof = StealthValueProof {
            value: other_value,
            knowledge_proof: ValueKnowledgeProof::Commitment {
                mask_knowledge_proof: sig.to_byte_type(),
            },
        };

        // Validate the proof
        let err = validate_value_proof(&commitment_bytes, None, &proof).unwrap_err();
        match err {
            ResourceError::UtxoBurnFailed { details, .. } => {
                assert_eq!(details, "Invalid mask knowledge proof");
            },
            _ => panic!("Unexpected error type {err}"),
        }
    }
}
