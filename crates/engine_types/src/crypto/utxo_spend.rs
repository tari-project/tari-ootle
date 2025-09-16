//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::RistrettoPublicKey;
use tari_template_lib::models::StealthInput;

use crate::{
    crypto::{messages, try_decode_to_signature},
    resource_container::ResourceError,
    ConvertFromByteType,
    UtxoOutput,
};

pub fn verify_utxo_spend_permission(utxo: &UtxoOutput, input: &StealthInput) -> Result<(), ResourceError> {
    let balance_proof = try_decode_to_signature(&input.owner_proof).ok_or_else(|| ResourceError::InvalidSpend {
        details: "Malformed ownership proof".to_string(),
    })?;

    let message = messages::stealth_ownership64(
        &utxo.owner_public_key,
        input.owner_proof.public_nonce(),
        &input.commitment,
        &utxo.output.public_nonce,
    );
    let signer_pk = RistrettoPublicKey::convert_from_byte_type(&utxo.owner_public_key).map_err(|_| {
        ResourceError::InvalidSpend {
            details: "Non-canonical compressed owner public key".to_string(),
        }
    })?;

    if !balance_proof.verify_raw_uniform(&signer_pk, &message) {
        return Err(ResourceError::InvalidSpend {
            details: format!("Invalid ownership proof for input with commitment {}", input.commitment),
        });
    }

    Ok(())
}
