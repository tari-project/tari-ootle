//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::FromByteType;
use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey, pedersen::PedersenCommitment},
    tari_utilities::ByteArray,
};
use tari_template_lib::types::{
    Amount,
    UtxoId,
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, Scalar32Bytes, StealthValueProof, ValueKnowledgeProof},
};

use crate::{
    crypto::{ElgamalVerifiableBalanceBytes, commit_amount, convert_amount_to_secret, messages},
    resource_container::ResourceError,
};

pub fn validate_value_proof(
    commitment_bytes: &PedersenCommitmentBytes,
    view_key: Option<&RistrettoPublicKey>,
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
            let Some(commit_amount) = commit_amount(&RistrettoSecretKey::default(), proof.value) else {
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
        ValueKnowledgeProof::ElgamalEncrypted {
            ref public_nonce_g,
            ref public_nonce_r,
            ref s_p,
        } => {
            validate_elgamal_knowledge_proof(
                commitment_bytes,
                view_key,
                elgamal_verifiable_balance,
                proof.value,
                public_nonce_g,
                public_nonce_r,
                s_p,
            )?;
        },
    }

    Ok(proof.value)
}

fn validate_elgamal_knowledge_proof(
    commitment_bytes: &PedersenCommitmentBytes,
    view_key: Option<&RistrettoPublicKey>,
    elgamal_verifiable_balance: Option<&ElgamalVerifiableBalanceBytes>,
    value: Amount,
    public_nonce_g: &RistrettoPublicKeyBytes,
    public_nonce_r: &RistrettoPublicKeyBytes,
    s_p: &Scalar32Bytes,
) -> Result<(), ResourceError> {
    // Viewable balances encrypt a u64 value, so a larger claimed value can never be honest. This also keeps
    // the bound consistent with the Commitment variant, which inherits it from `commit_amount`.
    if value > u64::MAX {
        return Err(ResourceError::UtxoBurnFailed {
            id: UtxoId::from(*commitment_bytes),
            details: "Value proof amount is too large".to_string(),
        });
    }

    let elgamal = elgamal_verifiable_balance.ok_or_else(|| ResourceError::UtxoBurnFailed {
        id: UtxoId::from(*commitment_bytes),
        details: "Utxo does not have a viewable balance".to_string(),
    })?;

    let view_key = view_key.ok_or_else(|| ResourceError::UtxoBurnFailed {
        id: UtxoId::from(*commitment_bytes),
        details: "Resource does not have a view key".to_string(),
    })?;

    let encrypted: RistrettoPublicKey =
        elgamal
            .encrypted
            .try_from_byte_type()
            .map_err(|e| ResourceError::UtxoBurnFailed {
                id: UtxoId::from(*commitment_bytes),
                details: format!("Invalid encrypted balance bytes: {}", e),
            })?;

    let elgamal_public_nonce: RistrettoPublicKey =
        elgamal
            .public_nonce
            .try_from_byte_type()
            .map_err(|e| ResourceError::UtxoBurnFailed {
                id: UtxoId::from(*commitment_bytes),
                details: format!("Invalid ElGamal public nonce bytes: {}", e),
            })?;

    let public_nonce_g: RistrettoPublicKey =
        public_nonce_g
            .try_from_byte_type()
            .map_err(|e| ResourceError::UtxoBurnFailed {
                id: UtxoId::from(*commitment_bytes),
                details: format!("Invalid public nonce (G) bytes: {}", e),
            })?;

    let public_nonce_r: RistrettoPublicKey =
        public_nonce_r
            .try_from_byte_type()
            .map_err(|e| ResourceError::UtxoBurnFailed {
                id: UtxoId::from(*commitment_bytes),
                details: format!("Invalid public nonce (R) bytes: {}", e),
            })?;

    let s_p = RistrettoSecretKey::from_canonical_bytes(s_p.as_bytes()).map_err(|e| ResourceError::UtxoBurnFailed {
        id: UtxoId::from(*commitment_bytes),
        details: format!("Invalid scalar s_p bytes: {}", e),
    })?;

    let challenge = messages::elgamal_value_proof64(
        commitment_bytes,
        view_key,
        &encrypted,
        &elgamal_public_nonce,
        &value,
        &public_nonce_g,
        &public_nonce_r,
    );
    let e = RistrettoSecretKey::from_uniform_bytes(&challenge)
        .expect("INVARIANT VIOLATION: RistrettoSecretKey::from_uniform_bytes and hash output length mismatch");

    // Check s.G ?= K_g + e.P
    if RistrettoPublicKey::from_secret_key(&s_p) != public_nonce_g + &e * view_key {
        return Err(ResourceError::UtxoBurnFailed {
            id: UtxoId::from(*commitment_bytes),
            details: "Invalid Elgamal encrypted value proof (s.G != K_g + e.P)".to_string(),
        });
    }

    // D = E - v.G is p.R exactly when v is the value encrypted for the view key
    let value = convert_amount_to_secret(&value);
    let value_g = RistrettoPublicKey::from_secret_key(&value);
    let d = encrypted - value_g;

    // Check s.R ?= K_r + e.D
    if &s_p * &elgamal_public_nonce != public_nonce_r + &e * d {
        return Err(ResourceError::UtxoBurnFailed {
            id: UtxoId::from(*commitment_bytes),
            details: "Invalid Elgamal encrypted value proof (s.R != K_r + e.D)".to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::keys::SecretKey;
    use tari_template_lib::types::crypto::{StealthValueProof, ValueKnowledgeProof};

    use super::*;

    #[test]
    fn it_proves_knowledge_of_the_value() {
        let mut rng = rand::rng();
        let mask = RistrettoSecretKey::random(&mut rng);
        let value = 100_321_123u128.into();
        let commitment = commit_amount(&mask, value).unwrap();
        let commitment_bytes = commitment.to_byte_type();

        // Create the proof of knowledge of the value
        let message = messages::value_proof_message(&commitment_bytes, &value);
        let sig = RistrettoSchnorr::sign(&mask, message, &mut rng).unwrap();

        let proof = StealthValueProof {
            value,
            knowledge_proof: ValueKnowledgeProof::Commitment {
                mask_knowledge_proof: sig.to_byte_type(),
            },
        };

        // Validate the proof
        let amount = validate_value_proof(&commitment_bytes, None, None, &proof).unwrap();
        assert_eq!(amount, value);
    }

    #[test]
    fn it_fails_if_the_value_differs() {
        let mut rng = rand::rng();
        let mask = RistrettoSecretKey::random(&mut rng);
        let value = 100_321_123u128.into();
        let commitment = commit_amount(&mask, value).unwrap();
        let commitment_bytes = commitment.to_byte_type();

        let other_value = value + Amount::ONE;

        // Create the proof of knowledge of the value
        let message = messages::value_proof_message(&commitment_bytes, &other_value);
        let sig = RistrettoSchnorr::sign(&mask, message, &mut rng).unwrap();

        let proof = StealthValueProof {
            value: other_value,
            knowledge_proof: ValueKnowledgeProof::Commitment {
                mask_knowledge_proof: sig.to_byte_type(),
            },
        };

        // Validate the proof
        let err = validate_value_proof(&commitment_bytes, None, None, &proof).unwrap_err();
        match err {
            ResourceError::UtxoBurnFailed { details, .. } => {
                assert_eq!(details, "Invalid mask knowledge proof");
            },
            _ => panic!("Unexpected error type {err}"),
        }
    }

    mod elgamal {
        use tari_template_lib::types::crypto::Scalar32Bytes;

        use super::*;

        struct ViewableUtxo {
            commitment_bytes: PedersenCommitmentBytes,
            balance_bytes: ElgamalVerifiableBalanceBytes,
            view_secret: RistrettoSecretKey,
            view_key: RistrettoPublicKey,
            encrypted: RistrettoPublicKey,
            elgamal_public_nonce: RistrettoPublicKey,
        }

        fn new_viewable_utxo(value: Amount) -> ViewableUtxo {
            let mut rng = rand::rng();
            let mask = RistrettoSecretKey::random(&mut rng);
            let view_secret = RistrettoSecretKey::random(&mut rng);
            let view_key = RistrettoPublicKey::from_secret_key(&view_secret);
            let elgamal_secret_nonce = RistrettoSecretKey::random(&mut rng);

            let commitment = commit_amount(&mask, value).unwrap();

            // E = v.G + r.P, R = r.G
            let encrypted = RistrettoPublicKey::from_secret_key(&convert_amount_to_secret(&value)) +
                &elgamal_secret_nonce * &view_key;
            let elgamal_public_nonce = RistrettoPublicKey::from_secret_key(&elgamal_secret_nonce);

            ViewableUtxo {
                commitment_bytes: commitment.to_byte_type(),
                balance_bytes: ElgamalVerifiableBalanceBytes {
                    encrypted: encrypted.to_byte_type(),
                    public_nonce: elgamal_public_nonce.to_byte_type(),
                },
                view_secret,
                view_key,
                encrypted,
                elgamal_public_nonce,
            }
        }

        /// Builds a DLEQ value proof over the given statement using `witness` as the claimed view private key. An
        /// honest prover passes the actual view private key; passing anything else models a forgery attempt.
        fn generate_proof(
            utxo: &ViewableUtxo,
            claimed_value: Amount,
            witness: &RistrettoSecretKey,
        ) -> StealthValueProof {
            let mut rng = rand::rng();
            let k = RistrettoSecretKey::random(&mut rng);
            let public_nonce_g = RistrettoPublicKey::from_secret_key(&k);
            let public_nonce_r = &k * &utxo.elgamal_public_nonce;

            let challenge = messages::elgamal_value_proof64(
                &utxo.commitment_bytes,
                &utxo.view_key,
                &utxo.encrypted,
                &utxo.elgamal_public_nonce,
                &claimed_value,
                &public_nonce_g,
                &public_nonce_r,
            );
            let e = RistrettoSecretKey::from_uniform_bytes(&challenge).unwrap();
            let s_p = &k + &e * witness;

            StealthValueProof {
                value: claimed_value,
                knowledge_proof: ValueKnowledgeProof::ElgamalEncrypted {
                    public_nonce_g: public_nonce_g.to_byte_type(),
                    public_nonce_r: public_nonce_r.to_byte_type(),
                    s_p: Scalar32Bytes::from_bytes(s_p.as_bytes()).unwrap(),
                },
            }
        }

        #[test]
        fn it_proves_the_value_of_a_viewable_balance() {
            let value = Amount::from(52_412u128);
            let utxo = new_viewable_utxo(value);
            let view_secret = utxo.view_secret.clone();

            let proof = generate_proof(&utxo, value, &view_secret);

            let amount = validate_value_proof(
                &utxo.commitment_bytes,
                Some(&utxo.view_key),
                Some(&utxo.balance_bytes),
                &proof,
            )
            .unwrap();
            assert_eq!(amount, value);
        }

        #[test]
        fn it_rejects_a_false_value_claimed_by_the_view_key_holder() {
            let value = Amount::from(52_412u128);
            let utxo = new_viewable_utxo(value);
            let view_secret = utxo.view_secret.clone();

            let proof = generate_proof(&utxo, value + Amount::ONE, &view_secret);

            validate_value_proof(
                &utxo.commitment_bytes,
                Some(&utxo.view_key),
                Some(&utxo.balance_bytes),
                &proof,
            )
            .unwrap_err();
        }

        #[test]
        fn it_rejects_a_proof_from_a_prover_without_the_view_key() {
            let value = Amount::from(52_412u128);
            let utxo = new_viewable_utxo(value);

            // A prover that does not hold the view private key claims an arbitrary value, including the true one
            let forged_witness = RistrettoSecretKey::random(&mut rand::rng());
            for claimed_value in [Amount::ONE, value] {
                let proof = generate_proof(&utxo, claimed_value, &forged_witness);
                validate_value_proof(
                    &utxo.commitment_bytes,
                    Some(&utxo.view_key),
                    Some(&utxo.balance_bytes),
                    &proof,
                )
                .unwrap_err();
            }
        }

        #[test]
        fn it_rejects_a_value_above_u64_max() {
            let value = Amount::from(1000u128);
            let utxo = new_viewable_utxo(value);
            let view_secret = utxo.view_secret.clone();

            let proof = generate_proof(&utxo, Amount::from(u128::from(u64::MAX)) + Amount::ONE, &view_secret);

            let err = validate_value_proof(
                &utxo.commitment_bytes,
                Some(&utxo.view_key),
                Some(&utxo.balance_bytes),
                &proof,
            )
            .unwrap_err();
            match err {
                ResourceError::UtxoBurnFailed { details, .. } => {
                    assert_eq!(details, "Value proof amount is too large");
                },
                _ => panic!("Unexpected error type {err}"),
            }
        }
    }
}
