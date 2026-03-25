//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use wasm_bindgen::prelude::*;

/// Called automatically when the WASM module is instantiated. Do not call directly.
#[wasm_bindgen(start)]
fn on_start() {
    #[cfg(feature = "debug")]
    console_error_panic_hook::set_once();
}

/// A generated keypair (raw bytes).
#[wasm_bindgen(getter_with_clone)]
pub struct KeypairResult {
    pub secret_key: Vec<u8>,
    pub public_key: Vec<u8>,
}

/// Result of a Schnorr signature operation (raw bytes).
#[wasm_bindgen(getter_with_clone)]
pub struct SchnorrSignatureResult {
    pub public_nonce: Vec<u8>,
    pub signature: Vec<u8>,
}

/// A pair of Ootle secret keys (owner + view).
#[wasm_bindgen(getter_with_clone)]
pub struct OotleSecretKey {
    /// The owner (spending) secret key bytes.
    pub owner_key: Vec<u8>,
    /// The view-only secret key bytes.
    pub view_key: Vec<u8>,
}

/// A pair of Ootle public keys derived from an OotleSecretKey.
#[wasm_bindgen(getter_with_clone)]
pub struct OotlePublicKey {
    /// The owner (spending) public key bytes.
    pub owner_key: Vec<u8>,
    /// The view-only public key bytes.
    pub view_key: Vec<u8>,
}

/// Parsed components of an Ootle address.
#[wasm_bindgen(getter_with_clone)]
pub struct ParsedOotleAddress {
    /// The owner (spending) public key bytes.
    pub owner_key: Vec<u8>,
    /// The view-only public key bytes.
    pub view_key: Vec<u8>,
    /// The network byte.
    pub network: u8,
    /// Optional pay reference / memo bytes.
    pub memo: Option<Vec<u8>>,
}

/// Seal a transaction (unsigned or unsealed JSON) with the seal signer's secret key.
///
/// Accepts either an `UnsignedTransactionV1` or `UnsealedTransactionV1` JSON string.
/// Returns the sealed `Transaction` as a JSON string.
#[wasm_bindgen(js_name = "sealTransaction")]
pub fn seal_transaction(tx_json: &str, seal_signer_secret_key: &[u8]) -> Result<String, JsError> {
    ootle_wasm_core::transaction::seal_transaction_json(tx_json, seal_signer_secret_key)
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Add a signer to a transaction (unsigned or unsealed JSON).
///
/// Accepts either an `UnsignedTransactionV1` or `UnsealedTransactionV1` JSON string.
/// Returns the `UnsealedTransactionV1` (with the new signature appended) as a JSON string.
#[wasm_bindgen(js_name = "addTransactionSigner")]
pub fn add_transaction_signer(
    tx_json: &str,
    signer_secret_key: &[u8],
    seal_signer_public_key: &[u8],
) -> Result<String, JsError> {
    ootle_wasm_core::transaction::add_transaction_signer_json(tx_json, signer_secret_key, seal_signer_public_key)
        .map_err(|e| JsError::new(&e.to_string()))
}

/// BOR-encode a Transaction (JSON string) → base64 string (TransactionEnvelope format).
#[wasm_bindgen(js_name = "borEncodeTransaction")]
pub fn bor_encode_transaction(transaction_json: &str) -> Result<String, JsError> {
    ootle_wasm_core::bor::bor_encode_transaction_json(transaction_json).map_err(|e| JsError::new(&e.to_string()))
}

/// Schnorr-sign a message with a secret key.
/// Returns { public_nonce: Uint8Array, signature: Uint8Array }.
#[wasm_bindgen(js_name = "schnorrSign")]
pub fn schnorr_sign(secret_key: &[u8], message: &[u8]) -> Result<SchnorrSignatureResult, JsError> {
    let result = ootle_wasm_core::sign::schnorr_sign(secret_key, message).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(SchnorrSignatureResult {
        public_nonce: result.public_nonce,
        signature: result.signature,
    })
}

/// Derive the public key from a secret key (both raw bytes).
#[wasm_bindgen(js_name = "publicKeyFromSecretKey")]
pub fn public_key_from_secret_key(secret_key: &[u8]) -> Result<Vec<u8>, JsError> {
    ootle_wasm_core::sign::public_key_from_secret_key(secret_key).map_err(|e| JsError::new(&e.to_string()))
}

/// Generate a new random Ristretto keypair.
/// Returns { secret_key: Uint8Array, public_key: Uint8Array }.
#[wasm_bindgen(js_name = "generateKeypair")]
pub fn generate_keypair() -> KeypairResult {
    let result = ootle_wasm_core::sign::generate_keypair();
    KeypairResult {
        secret_key: result.secret_key,
        public_key: result.public_key,
    }
}

/// Hash an UnsignedTransactionV1 (JSON string) for signing.
/// Returns the 64-byte signing message that must be Schnorr-signed.
///
/// `seal_signer_public_key` is the raw bytes of the seal signer's public key (account owner).
#[wasm_bindgen(js_name = "hashUnsignedTransaction")]
pub fn hash_unsigned_transaction(unsigned_tx_json: &str, seal_signer_public_key: &[u8]) -> Result<Vec<u8>, JsError> {
    ootle_wasm_core::hash::hash_unsigned_transaction_json(unsigned_tx_json, seal_signer_public_key)
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Generate a new random pair of Ootle secret keys (owner + view).
/// Returns { owner_key: Uint8Array, view_key: Uint8Array }.
#[wasm_bindgen(js_name = "generateOotleSecretKey")]
pub fn generate_ootle_secret_key() -> OotleSecretKey {
    let result = ootle_wasm_core::address::generate_ootle_secret_key();
    OotleSecretKey {
        owner_key: result.owner_key,
        view_key: result.view_key,
    }
}

/// Derive the Ootle public keys from a pair of secret keys.
/// Returns { owner_key: Uint8Array, view_key: Uint8Array }.
#[wasm_bindgen(js_name = "ootlePublicKeyFromSecretKey")]
pub fn ootle_public_key_from_secret_key(owner_key: &[u8], view_key: &[u8]) -> Result<OotlePublicKey, JsError> {
    let secret = ootle_wasm_core::address::OotleSecretKeyResult {
        owner_key: owner_key.to_vec(),
        view_key: view_key.to_vec(),
    };
    let result = ootle_wasm_core::address::ootle_public_key_from_secret_key(&secret)
        .map_err(|e| JsError::new(&e.to_string()))?;
    Ok(OotlePublicKey {
        owner_key: result.owner_key,
        view_key: result.view_key,
    })
}

/// Generate an Ootle address (bech32m string) from public keys.
///
/// `network` is the network byte (0x00 = MainNet, 0x10 = LocalNet, 0x26 = Esmeralda, etc.).
/// `memo` is an optional pay reference (max 64 bytes).
#[wasm_bindgen(js_name = "generateOotleAddress")]
pub fn generate_ootle_address(
    owner_public_key: &[u8],
    view_public_key: &[u8],
    network: u8,
    memo: Option<Vec<u8>>,
) -> Result<String, JsError> {
    ootle_wasm_core::address::generate_ootle_address(owner_public_key, view_public_key, network, memo.as_deref())
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Parse a bech32m Ootle address string into its components.
/// Returns { owner_key: Uint8Array, view_key: Uint8Array, network: number, memo: Uint8Array | undefined }.
#[wasm_bindgen(js_name = "parseOotleAddress")]
pub fn parse_ootle_address(address: &str) -> Result<ParsedOotleAddress, JsError> {
    let result = ootle_wasm_core::address::parse_ootle_address(address).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(ParsedOotleAddress {
        owner_key: result.owner_key,
        view_key: result.view_key,
        network: result.network,
        memo: result.memo,
    })
}
