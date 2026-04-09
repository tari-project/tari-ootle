//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Wallet for managing keys and signing transactions.
//!
//! [`OotleWallet`] holds one or more key providers and handles transaction signing,
//! authorization, stealth proof generation, and input decryption.
//!
//! ```rust,ignore
//! use ootle_rs::{key_provider::PrivateKeyProvider, wallet::OotleWallet, Network};
//!
//! let signer = PrivateKeyProvider::random(Network::LocalNet);
//! let wallet = OotleWallet::from(signer);
//! ```

mod error;
mod none;
mod ootle;
mod stealth;
mod traits;

pub use error::*;
pub use none::*;
pub use ootle::*;
pub use stealth::*;
pub use traits::*;
