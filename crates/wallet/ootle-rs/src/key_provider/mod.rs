//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Cryptographic key providers for output mask generation and key derivation.
//!
//! The main type is [`PrivateKeyProvider`] (alias [`PrivateKeySigner`]), a local key
//! provider backed by a Ristretto secret key that can sign transactions, decrypt
//! stealth inputs, and derive various stealth secrets.

mod error;
mod local;
mod traits;

pub use error::*;
pub use local::*;
pub use traits::*;
