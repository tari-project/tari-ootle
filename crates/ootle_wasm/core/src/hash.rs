//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::{ristretto::RistrettoPublicKey, tari_utilities::ByteArray};
use tari_ootle_transaction::{TransactionSignature, UnsignedTransactionV1};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::error::OotleWasmError;

/// Hash an UnsignedTransactionV1 for signing, producing the 64-byte message that signers commit to.
///
/// This delegates to `TransactionSignature::create_message_v1` to ensure byte-identical hashing
/// with the Rust tari transaction crate.
///
/// `seal_signer_public_key_hex` is the hex-encoded public key of the seal signer (the account owner).
pub fn hash_unsigned_transaction(
    transaction: &UnsignedTransactionV1,
    seal_signer_public_key_hex: &str,
) -> Result<Vec<u8>, OotleWasmError> {
    let seal_signer = public_key_bytes_from_hex(seal_signer_public_key_hex)?;
    let hash = TransactionSignature::create_message_v1(&seal_signer, transaction);
    Ok(hash.to_vec())
}

/// Hash an UnsignedTransactionV1 from a JSON string for signing.
pub fn hash_unsigned_transaction_json(
    unsigned_tx_json: &str,
    seal_signer_public_key_hex: &str,
) -> Result<Vec<u8>, OotleWasmError> {
    let tx: UnsignedTransactionV1 = serde_json::from_str(unsigned_tx_json)?;
    hash_unsigned_transaction(&tx, seal_signer_public_key_hex)
}

fn public_key_bytes_from_hex(hex_str: &str) -> Result<RistrettoPublicKeyBytes, OotleWasmError> {
    let bytes = hex::decode(hex_str)?;
    // Validate that it's a valid public key by attempting to deserialize
    let _pk = RistrettoPublicKey::from_canonical_bytes(&bytes)
        .map_err(|e| OotleWasmError::InvalidPublicKey(e.to_string()))?;
    let bytes_array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| OotleWasmError::InvalidPublicKey("expected 32 bytes".to_string()))?;
    Ok(RistrettoPublicKeyBytes::from(bytes_array))
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use rand::rngs::OsRng;
    use tari_crypto::{
        keys::{PublicKey, SecretKey},
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
        tari_utilities::ByteArray,
    };
    use tari_ootle_transaction::UnsignedTransactionV1;

    use super::*;

    #[test]
    fn hash_matches_transaction_crate() {
        let secret = RistrettoSecretKey::random(&mut OsRng);
        let public_key = RistrettoPublicKey::from_secret_key(&secret);
        let public_key_hex = hex::encode(public_key.as_bytes());
        let seal_signer_bytes: RistrettoPublicKeyBytes = public_key.to_byte_type();

        let tx = UnsignedTransactionV1::new(0u8, vec![], vec![], Default::default(), None, None, false);

        // Hash via our function
        let our_hash = hash_unsigned_transaction(&tx, &public_key_hex).unwrap();

        // Hash via the transaction crate directly
        let expected = TransactionSignature::create_message_v1(&seal_signer_bytes, &tx);

        assert_eq!(our_hash, expected.to_vec());
    }

    #[test]
    fn hash_from_json_round_trip() {
        let secret = RistrettoSecretKey::random(&mut OsRng);
        let public_key = RistrettoPublicKey::from_secret_key(&secret);
        let public_key_hex = hex::encode(public_key.as_bytes());

        let tx = UnsignedTransactionV1::new(0u8, vec![], vec![], Default::default(), None, None, false);
        let json = serde_json::to_string(&tx).unwrap();

        let hash_from_struct = hash_unsigned_transaction(&tx, &public_key_hex).unwrap();
        let hash_from_json = hash_unsigned_transaction_json(&json, &public_key_hex).unwrap();

        assert_eq!(hash_from_struct, hash_from_json);
    }
}
