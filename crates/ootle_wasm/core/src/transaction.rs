//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::Deserialize;
use tari_ootle_transaction::{UnsealedTransactionV1, UnsignedTransactionV1};

use crate::{error::OotleWasmError, hash::public_key_bytes_from_bytes, keys::secret_key_from_bytes};

/// An unsigned or unsealed transaction parsed from JSON.
#[derive(Deserialize)]
#[serde(untagged)]
enum TransactionInput {
    Unsealed(UnsealedTransactionV1),
    Unsigned(UnsignedTransactionV1),
}

impl TransactionInput {
    fn into_unsealed(self) -> UnsealedTransactionV1 {
        match self {
            Self::Unsigned(tx) => UnsealedTransactionV1::new(tx, vec![]),
            Self::Unsealed(tx) => tx,
        }
    }
}

fn parse_transaction_json(tx_json: &str) -> Result<TransactionInput, OotleWasmError> {
    serde_json::from_str(tx_json).map_err(Into::into)
}

/// Seal a transaction (accepts unsigned or unsealed JSON) with the seal signer's secret key.
///
/// Returns the sealed `Transaction` as a JSON string.
pub fn seal_transaction_json(tx_json: &str, seal_signer_secret_key: &[u8]) -> Result<String, OotleWasmError> {
    let unsealed = parse_transaction_json(tx_json)?.into_unsealed();
    let secret = secret_key_from_bytes(seal_signer_secret_key)?;
    let sealed = unsealed.seal(&secret);
    Ok(serde_json::to_string(&sealed)?)
}

/// Add a signer to a transaction (accepts unsigned or unsealed JSON).
///
/// Returns the unsealed transaction (with the new signature appended) as a JSON string.
pub fn add_transaction_signer_json(
    tx_json: &str,
    signer_secret_key: &[u8],
    seal_signer_public_key: &[u8],
) -> Result<String, OotleWasmError> {
    let unsealed = parse_transaction_json(tx_json)?.into_unsealed();
    let secret = secret_key_from_bytes(signer_secret_key)?;
    let seal_signer = public_key_bytes_from_bytes(seal_signer_public_key)?;
    let unsealed = unsealed.add_signer(&seal_signer, &secret);
    Ok(serde_json::to_string(&unsealed)?)
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{
        keys::{PublicKey, SecretKey},
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
        tari_utilities::ByteArray,
    };
    use tari_ootle_transaction::{Transaction, UnsignedTransactionV1};

    use super::*;

    fn make_unsigned_tx() -> UnsignedTransactionV1 {
        UnsignedTransactionV1::new(0u8, vec![], vec![], Default::default(), None, None, false)
    }

    #[test]
    fn seal_unsigned_transaction() {
        let secret = RistrettoSecretKey::random(&mut rand::rng());
        let unsigned_json = serde_json::to_string(&make_unsigned_tx()).unwrap();

        let sealed_json = seal_transaction_json(&unsigned_json, secret.as_bytes()).unwrap();
        let sealed: Transaction = serde_json::from_str(&sealed_json).unwrap();
        assert!(sealed.verify_all_signatures());
    }

    #[test]
    fn seal_unsealed_transaction() {
        let seal_secret = RistrettoSecretKey::random(&mut rand::rng());
        let signer_secret = RistrettoSecretKey::random(&mut rand::rng());
        let seal_pk = RistrettoPublicKey::from_secret_key(&seal_secret);

        // First add a signer to get an unsealed tx
        let unsigned_json = serde_json::to_string(&make_unsigned_tx()).unwrap();
        let unsealed_json =
            add_transaction_signer_json(&unsigned_json, signer_secret.as_bytes(), seal_pk.as_bytes()).unwrap();

        // Now seal the unsealed tx
        let sealed_json = seal_transaction_json(&unsealed_json, seal_secret.as_bytes()).unwrap();
        let sealed: Transaction = serde_json::from_str(&sealed_json).unwrap();
        assert!(sealed.verify_all_signatures());
    }

    #[test]
    fn add_signer_to_unsigned_transaction() {
        let seal_secret = RistrettoSecretKey::random(&mut rand::rng());
        let seal_pk = RistrettoPublicKey::from_secret_key(&seal_secret);
        let signer_secret = RistrettoSecretKey::random(&mut rand::rng());

        let unsigned_json = serde_json::to_string(&make_unsigned_tx()).unwrap();
        let unsealed_json =
            add_transaction_signer_json(&unsigned_json, signer_secret.as_bytes(), seal_pk.as_bytes()).unwrap();

        let unsealed: UnsealedTransactionV1 = serde_json::from_str(&unsealed_json).unwrap();
        assert_eq!(unsealed.signatures().len(), 1);
        assert!(unsealed.verify_all_signatures(&seal_pk.to_byte_type()));
    }

    #[test]
    fn add_multiple_signers() {
        let seal_secret = RistrettoSecretKey::random(&mut rand::rng());
        let seal_pk = RistrettoPublicKey::from_secret_key(&seal_secret);
        let signer1 = RistrettoSecretKey::random(&mut rand::rng());
        let signer2 = RistrettoSecretKey::random(&mut rand::rng());

        let unsigned_json = serde_json::to_string(&make_unsigned_tx()).unwrap();

        // Add first signer (unsigned → unsealed)
        let unsealed_json =
            add_transaction_signer_json(&unsigned_json, signer1.as_bytes(), seal_pk.as_bytes()).unwrap();

        // Add second signer (unsealed → unsealed)
        let unsealed_json =
            add_transaction_signer_json(&unsealed_json, signer2.as_bytes(), seal_pk.as_bytes()).unwrap();

        // Seal and verify
        let sealed_json = seal_transaction_json(&unsealed_json, seal_secret.as_bytes()).unwrap();
        let sealed: Transaction = serde_json::from_str(&sealed_json).unwrap();
        assert!(sealed.verify_all_signatures());
        assert_eq!(sealed.signatures().len(), 2);
    }
}
