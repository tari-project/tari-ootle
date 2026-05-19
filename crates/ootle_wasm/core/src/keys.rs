//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Shared byte-parsing helpers for the WASM ABI boundary.
//!
//! The WASM exports accept fixed-size scalars and public keys as raw byte slices for performance, and
//! variable-length values (hex strings, JSON blobs) as `&str`. These helpers centralise the conversion
//! and validation logic so the individual modules only deal with strongly-typed values.

use tari_crypto::{
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArray,
};
use tari_template_lib_types::crypto::PedersenCommitmentBytes;

use crate::error::OotleWasmError;

/// Parse a Ristretto secret key (32-byte canonical scalar) from raw bytes.
pub(crate) fn secret_key_from_bytes(bytes: &[u8]) -> Result<RistrettoSecretKey, OotleWasmError> {
    RistrettoSecretKey::from_canonical_bytes(bytes).map_err(|e| OotleWasmError::InvalidSecretKey(e.to_string()))
}

/// Parse a Ristretto public key (32-byte canonical curve point) from raw bytes.
pub(crate) fn public_key_from_bytes(bytes: &[u8]) -> Result<RistrettoPublicKey, OotleWasmError> {
    RistrettoPublicKey::from_canonical_bytes(bytes).map_err(|e| OotleWasmError::InvalidPublicKey(e.to_string()))
}

/// Parse a Pedersen commitment (32-byte byte type) from raw bytes.
pub(crate) fn commitment_bytes_from_bytes(bytes: &[u8]) -> Result<PedersenCommitmentBytes, OotleWasmError> {
    PedersenCommitmentBytes::from_bytes(bytes).map_err(|e| OotleWasmError::InvalidCommitment(e.to_string()))
}
