//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use wasm_bindgen::prelude::*;

/// Called automatically when the WASM module is instantiated. Do not call directly.
#[wasm_bindgen(start)]
fn on_start() {
    #[cfg(feature = "debug")]
    console_error_panic_hook::set_once();
}

/// A generated keypair with hex-encoded keys.
#[wasm_bindgen(getter_with_clone)]
pub struct KeypairResult {
    pub secret_key: String,
    pub public_key: String,
}

/// Result of a Schnorr signature operation (hex-encoded).
#[wasm_bindgen(getter_with_clone)]
pub struct SchnorrSignatureResult {
    pub public_nonce: String,
    pub signature: String,
}

/// BOR-encode a Transaction (JSON string) → base64 string (TransactionEnvelope format).
#[wasm_bindgen(js_name = "borEncodeTransaction")]
pub fn bor_encode_transaction(transaction_json: &str) -> Result<String, JsError> {
    ootle_wasm_core::bor::bor_encode_transaction_json(transaction_json).map_err(|e| JsError::new(&e.to_string()))
}

/// Schnorr-sign a message with a secret key.
/// Returns a JS object { public_nonce: string, signature: string } (hex-encoded).
#[wasm_bindgen(js_name = "schnorrSign")]
pub fn schnorr_sign(secret_key_hex: &str, message: &[u8]) -> Result<SchnorrSignatureResult, JsError> {
    let result =
        ootle_wasm_core::sign::schnorr_sign(secret_key_hex, message).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(SchnorrSignatureResult {
        public_nonce: result.public_nonce,
        signature: result.signature,
    })
}

/// Derive the public key from a secret key (hex-encoded).
#[wasm_bindgen(js_name = "publicKeyFromSecretKey")]
pub fn public_key_from_secret_key(secret_key_hex: &str) -> Result<String, JsError> {
    ootle_wasm_core::sign::public_key_from_secret_key(secret_key_hex).map_err(|e| JsError::new(&e.to_string()))
}

/// Generate a new random Ristretto keypair.
/// Returns a JS object { secret_key: string, public_key: string } (hex-encoded).
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
/// `seal_signer_public_key_hex` is the hex-encoded public key of the seal signer (account owner).
#[wasm_bindgen(js_name = "hashUnsignedTransaction")]
pub fn hash_unsigned_transaction(unsigned_tx_json: &str, seal_signer_public_key_hex: &str) -> Result<Vec<u8>, JsError> {
    ootle_wasm_core::hash::hash_unsigned_transaction_json(unsigned_tx_json, seal_signer_public_key_hex)
        .map_err(|e| JsError::new(&e.to_string()))
}
