//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use tari_crypto::{ristretto::RistrettoPublicKey, tari_utilities::ByteArray};
use tari_ootle_transaction::{
    AuthorizationContext,
    AuthorizerSet,
    TransactionSealSignature,
    TransactionSignature,
    UnsealedTransactionV1,
    UnsignedTransactionV1,
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{error::OotleWasmError, transaction::parse_unsealed_transaction_json};

/// Hash an UnsignedTransactionV1 for signing, producing the 64-byte message that signers commit to.
///
/// This delegates to `TransactionSignature::create_message_v1` to ensure byte-identical hashing
/// with the Rust tari transaction crate.
///
/// `seal_signer_public_key` is the raw bytes of the seal signer's public key (the account owner).
/// `authorizer_public_keys` is the raw 32-byte public key of every authorizer, concatenated in any
/// order: each authorization signs over the whole set, so it is fixed before anyone signs.
pub fn hash_unsigned_transaction(
    transaction: &UnsignedTransactionV1,
    seal_signer_public_key: &[u8],
    authorizer_public_keys: &[u8],
) -> Result<Vec<u8>, OotleWasmError> {
    let seal_signer = public_key_bytes_from_bytes(seal_signer_public_key)?;
    let authorizers = authorizer_set_from_bytes(authorizer_public_keys)?;
    let hash =
        TransactionSignature::create_message_v1(AuthorizationContext::new(&seal_signer, &authorizers), transaction);
    Ok(hash.as_bytes().to_vec())
}

/// Hash a transaction (accepts unsigned or unsealed JSON) for an authorization signature.
///
/// The authorization message excludes attached signatures, so unsigned and unsealed JSON of the same
/// transaction hash identically.
pub fn hash_unsigned_transaction_json(
    tx_json: &str,
    seal_signer_public_key: &[u8],
    authorizer_public_keys: &[u8],
) -> Result<Vec<u8>, OotleWasmError> {
    let tx = parse_unsealed_transaction_json(tx_json)?;
    hash_unsigned_transaction(
        tx.unsigned_transaction(),
        seal_signer_public_key,
        authorizer_public_keys,
    )
}

/// Parses concatenated raw 32-byte public keys into an authorizer set.
pub(crate) fn authorizer_set_from_bytes(bytes: &[u8]) -> Result<AuthorizerSet, OotleWasmError> {
    let key_len = RistrettoPublicKeyBytes::length();
    if !bytes.len().is_multiple_of(key_len) {
        return Err(OotleWasmError::InvalidAuthorizerKeys(bytes.len()));
    }
    bytes.chunks_exact(key_len).map(public_key_bytes_from_bytes).collect()
}

/// Hash an UnsealedTransactionV1 for sealing, producing the 64-byte message the seal signer commits to.
///
/// The seal message covers every authorization signature, so all of them must be attached before this
/// hash is taken.
pub fn hash_unsealed_transaction(transaction: &UnsealedTransactionV1) -> Vec<u8> {
    TransactionSealSignature::create_message_v1(transaction)
        .as_bytes()
        .to_vec()
}

/// Hash a transaction (accepts unsigned or unsealed JSON) for sealing.
pub fn hash_unsealed_transaction_json(tx_json: &str) -> Result<Vec<u8>, OotleWasmError> {
    let tx = parse_unsealed_transaction_json(tx_json)?;
    Ok(hash_unsealed_transaction(&tx))
}

pub(crate) fn public_key_bytes_from_bytes(bytes: &[u8]) -> Result<RistrettoPublicKeyBytes, OotleWasmError> {
    // Validate that it's a valid curve point
    let pk =
        RistrettoPublicKey::from_canonical_bytes(bytes).map_err(|e| OotleWasmError::InvalidPublicKey(e.to_string()))?;
    Ok(pk.to_byte_type())
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{
        keys::{PublicKey, SecretKey},
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
        tari_utilities::ByteArray,
    };
    use tari_engine_types::Epoch;
    use tari_ootle_transaction::UnsignedTransactionV1;

    use super::*;

    #[test]
    fn hash_matches_transaction_crate() {
        let secret = RistrettoSecretKey::random(&mut rand::rng());
        let public_key = RistrettoPublicKey::from_secret_key(&secret);
        let public_key_bytes = public_key.as_bytes().to_vec();
        let seal_signer_bytes: RistrettoPublicKeyBytes = public_key.to_byte_type();

        let tx = UnsignedTransactionV1::new(0u8, vec![], vec![], Default::default(), None, Epoch(1), false);

        let (_, authorizer) = RistrettoPublicKey::random_keypair(&mut rand::rng());
        let authorizers: AuthorizerSet = [authorizer.to_byte_type()].into_iter().collect();

        // Hash via our function
        let our_hash = hash_unsigned_transaction(&tx, &public_key_bytes, authorizer.as_bytes()).unwrap();

        // Hash via the transaction crate directly
        let expected =
            TransactionSignature::create_message_v1(AuthorizationContext::new(&seal_signer_bytes, &authorizers), &tx);

        assert_eq!(our_hash, expected.as_bytes().to_vec());
    }

    #[test]
    fn hash_from_json_round_trip() {
        let secret = RistrettoSecretKey::random(&mut rand::rng());
        let public_key = RistrettoPublicKey::from_secret_key(&secret);
        let public_key_bytes = public_key.as_bytes().to_vec();

        let tx = UnsignedTransactionV1::new(0u8, vec![], vec![], Default::default(), None, Epoch(1), false);
        let json = serde_json::to_string(&tx).unwrap();

        let hash_from_struct = hash_unsigned_transaction(&tx, &public_key_bytes, &public_key_bytes).unwrap();
        let hash_from_json = hash_unsigned_transaction_json(&json, &public_key_bytes, &public_key_bytes).unwrap();

        assert_eq!(hash_from_struct, hash_from_json);
    }

    /// The set is unordered on input and binds every key.
    #[test]
    fn hash_binds_the_authorizer_set_in_any_input_order() {
        let tx = UnsignedTransactionV1::new(0u8, vec![], vec![], Default::default(), None, Epoch(1), false);
        let (_, seal) = RistrettoPublicKey::random_keypair(&mut rand::rng());
        let (_, a) = RistrettoPublicKey::random_keypair(&mut rand::rng());
        let (_, b) = RistrettoPublicKey::random_keypair(&mut rand::rng());
        let ab = [a.as_bytes(), b.as_bytes()].concat();
        let ba = [b.as_bytes(), a.as_bytes()].concat();

        let hash_ab = hash_unsigned_transaction(&tx, seal.as_bytes(), &ab).unwrap();
        assert_eq!(hash_ab, hash_unsigned_transaction(&tx, seal.as_bytes(), &ba).unwrap());
        assert_ne!(
            hash_ab,
            hash_unsigned_transaction(&tx, seal.as_bytes(), a.as_bytes()).unwrap()
        );

        let err = hash_unsigned_transaction(&tx, seal.as_bytes(), &ab[..40]).unwrap_err();
        assert!(matches!(err, OotleWasmError::InvalidAuthorizerKeys(40)));
    }

    #[test]
    fn seal_hash_matches_transaction_crate() {
        let tx = UnsealedTransactionV1::new(
            UnsignedTransactionV1::new(0u8, vec![], vec![], Default::default(), None, Epoch(1), false),
            vec![],
        );
        let json = serde_json::to_string(&tx).unwrap();

        let expected = TransactionSealSignature::create_message_v1(&tx);
        assert_eq!(hash_unsealed_transaction(&tx), expected.as_bytes().to_vec());
        assert_eq!(
            hash_unsealed_transaction_json(&json).unwrap(),
            expected.as_bytes().to_vec()
        );
    }

    #[test]
    fn seal_hash_accepts_unsigned_json() {
        let unsigned = UnsignedTransactionV1::new(0u8, vec![], vec![], Default::default(), None, Epoch(1), false);
        let unsigned_json = serde_json::to_string(&unsigned).unwrap();
        let unsealed_json = serde_json::to_string(&UnsealedTransactionV1::new(unsigned, vec![])).unwrap();

        assert_eq!(
            hash_unsealed_transaction_json(&unsigned_json).unwrap(),
            hash_unsealed_transaction_json(&unsealed_json).unwrap()
        );
    }
}
