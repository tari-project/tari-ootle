//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Pre-flight validation for a full `StealthTransferStatement` envelope.

use tari_engine_types::stealth::validate_transfer as crypto_validate_transfer;
use tari_template_lib_types::stealth::StealthTransferStatement;

use crate::{error::OotleWasmError, keys::public_key_from_bytes};

/// Run the same validation the engine performs on a `StealthTransferStatement`: structural sanity,
/// commitment well-formedness, range and balance-proof verification.
///
/// `view_key` (optional) is the resource view-key public key — required when the resource has a viewable
/// balance enabled. Pass `None` for resources without a view key.
///
/// Returns `Ok(())` on a valid statement, or an error describing the validation failure.
pub fn validate_stealth_transfer(transfer_json: &str, view_key: Option<&[u8]>) -> Result<(), OotleWasmError> {
    let transfer: StealthTransferStatement = serde_json::from_str(transfer_json)?;
    let view_key = view_key.map(public_key_from_bytes).transpose()?;

    crypto_validate_transfer(&transfer, view_key.as_ref())
        .map_err(|e| OotleWasmError::StealthValidation(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::iter;

    use ootle_byte_type::ToByteType;
    use tari_crypto::{
        keys::{PublicKey, SecretKey},
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    };
    use tari_ootle_wallet_crypto::{
        MaskAndValue,
        OutputWitness,
        StealthInputWitness,
        StealthOutputWitness,
        stealth::create_transfer_statement,
    };
    use tari_template_lib_types::{Amount, EncryptedData, crypto::UtxoTag, stealth::SpendCondition};

    use super::*;

    #[test]
    fn validates_a_well_formed_transfer() {
        let mut rng = rand::rng();
        let input_mask = RistrettoSecretKey::random(&mut rng);
        let output_mask = RistrettoSecretKey::random(&mut rng);
        let owner_pk = RistrettoPublicKey::from_secret_key(&output_mask);

        let inputs = vec![StealthInputWitness {
            mask_and_value: MaskAndValue::new(500, input_mask),
        }];
        let outputs = vec![StealthOutputWitness {
            witness: OutputWitness {
                amount: 500,
                mask: output_mask,
                sender_public_nonce: owner_pk.clone(),
                minimum_value_promise: 0,
                encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                resource_view_key: None,
            },
            spend_condition: SpendCondition::Signed(owner_pk.to_byte_type()),
            tag: UtxoTag::new(0),
        }];
        let transfer = create_transfer_statement(inputs, Amount::zero(), outputs.iter(), Amount::zero()).unwrap();
        let json = serde_json::to_string(&transfer).unwrap();

        validate_stealth_transfer(&json, None).unwrap();
    }

    #[test]
    fn rejects_a_noop_transfer() {
        let transfer = create_transfer_statement(iter::empty(), Amount::zero(), iter::empty(), Amount::zero()).unwrap();
        let json = serde_json::to_string(&transfer).unwrap();
        let err = validate_stealth_transfer(&json, None).unwrap_err();
        assert!(matches!(err, OotleWasmError::StealthValidation(_)));
    }
}
