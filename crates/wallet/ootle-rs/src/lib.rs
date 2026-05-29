// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

//! # ootle.rs
//!
//! A Rust library for interacting with the [Tari Ootle](https://www.tari.com/) (Layer 2) network.
//!
//! `ootle-rs` provides a high-level, type-safe API modelled after
//! [alloy-rs](https://github.com/alloy-rs/alloy), using the familiar `Provider`, `Wallet`,
//! and `Signer` architecture. It supports public and confidential (stealth) transfers,
//! balance queries, template invocation, and real-time event streaming.
//!
//! # Quick start
//!
//! Connect to a local Ootle indexer, create a wallet, fund it from the faucet, and send
//! a public transfer:
//!
//! ```rust,no_run
//! use ootle_rs::{
//!     address,
//!     builtin_templates::{account::IAccount, faucet::IFaucet},
//!     key_provider::PrivateKeyProvider,
//!     provider::ProviderBuilder,
//!     wallet::OotleWallet,
//!     Network, TransactionRequest,
//! };
//! use tari_template_lib_types::constants::TARI_TOKEN;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let network = Network::LocalNet;
//!
//! // 1. Create a wallet backed by a random keypair
//! let signer = PrivateKeyProvider::random(network);
//! let wallet = OotleWallet::from(signer);
//!
//! // 2. Connect to the indexer
//! let provider = ProviderBuilder::new()
//!     .wallet(wallet)
//!     .connect("http://127.0.0.1:12500")
//!     .await?;
//!
//! // 3. Fund our account from the faucet
//! let faucet_tx = IFaucet::new(&provider)
//!     .pay_fee(1000u64)
//!     .take_free_coins(500_000_000u64)
//!     .prepare()
//!     .await?;
//!
//! let faucet_tx = TransactionRequest::default()
//!     .with_transaction(faucet_tx)
//!     .build(provider.wallet())
//!     .await?;
//! provider.send_transaction(faucet_tx).await?.watch().await?;
//!
//! // 4. Transfer tokens to a recipient
//! let recipient = address!("otl_loc_10mc0v2lyy43kldl0ft4c2x5pe7j0ckduv8zej6jgr2z2g9m07fz7gl96ar5wwgu0qu0atmr5tl53ye7n38xr5u7ytlmudq0ruxcau0gge7rxk");
//!
//! let unsigned_tx = IAccount::new(&provider)
//!     .pay_fee(1000u64)
//!     .public_transfer(&recipient, TARI_TOKEN, 1_000_000u64)
//!     .prepare()
//!     .await?;
//!
//! let tx = TransactionRequest::default()
//!     .with_transaction(unsigned_tx)
//!     .build(provider.wallet())
//!     .await?;
//!
//! let outcome = provider.send_transaction(tx).await?.watch().await?;
//! println!("Transaction confirmed: {:?}", outcome);
//! # Ok(())
//! # }
//! ```
//!
//! # Architecture
//!
//! The crate follows the layered design of alloy:
//!
//! - **[`provider`]** — the main entry point for network interaction. [`provider::ProviderBuilder`] connects to an
//!   Ootle indexer and provides methods for sending transactions, resolving inputs, and querying substates. The
//!   [`provider::Provider`] and [`provider::WalletProvider`] traits define the interface.
//!
//! - **[`wallet`]** — manages keys and signing. [`wallet::OotleWallet`] holds one or more key providers and handles
//!   transaction signing, authorization, and stealth proof generation.
//!
//! - **[`key_provider`]** — cryptographic key abstractions for output mask generation and Diffie-Hellman key
//!   derivation. [`key_provider::PrivateKeyProvider`] is the default implementation backed by a Ristretto secret key.
//!
//! - **[`builtin_templates`]** — ergonomic builders for the built-in Ootle templates:
//!   - [`builtin_templates::account::IAccount`] — public transfers, fee payment, template publishing.
//!   - [`builtin_templates::faucet::IFaucet`] — claim free testnet tokens.
//!   - [`builtin_templates::component`] — generic component/template invocation with the [`ootle_template!`] macro for
//!     type-safe method calls.
//!
//! - **[`stealth`]** — confidential transfer support. [`stealth::StealthTransfer`] builds stealth transfer statements
//!   with input/output commitments, encrypted memos, and change handling.
//!
//! - **[`claim_burn`]** — claim Layer 1 (minotari) burns. [`claim_burn::ClaimBurn`] mints burned funds into a
//!   confidential UTXO and spends it into a stealth output owned by the claiming account.
//!
//! - **[`transaction`]** — transaction signing traits ([`transaction::TransactionSigner`],
//!   [`transaction::TransactionSealSigner`]) and the `TransactionRequest` builder for constructing signed transactions
//!   ready for submission.
//!
//! # Type-safe template invocation
//!
//! The [`ootle_template!`] macro generates typed wrappers for custom templates:
//!
//! ```rust,ignore
//! use ootle_rs::ootle_template;
//! use tari_template_lib_types::Amount;
//!
//! ootle_template! {
//!     template StableCoin {
//!         fn instantiate(initial_supply: Amount);
//!         fn mint(&mut self, amount: Amount);
//!         fn balance(&self) -> Amount;
//!     }
//! }
//!
//! // Call a constructor
//! let tx = StableCoin::for_template(template_addr, &provider)
//!     .instantiate(Amount::new(1_000_000))
//!     .pay_fee(1000)
//!     .prepare()
//!     .await?;
//!
//! // Call a component method
//! let tx = StableCoin::for_component(component_addr, &provider)
//!     .mint(Amount::new(500))
//!     .pay_fee(1000)
//!     .prepare()
//!     .await?;
//! ```
//!
//! # Features
//!
//! - **`mmap-value-lookup`** — enables memory-mapped pregenerated lookup tables for fast ElGamal UTXO value decryption.
//!   Without this feature, values are decrypted via on-the-fly brute-force which is significantly slower.

pub mod builtin_templates;
pub mod claim_burn;
pub mod key_provider;
pub mod provider;
pub mod signer;
pub mod transaction;
pub mod wallet;

#[macro_use]
pub mod macros;

mod helpers;
pub mod keys;
pub mod stealth;
mod types;

// Re-export the address macro from the ootle_address crate
pub use helpers::*;
pub use tari_ootle_address::{Network, address};
pub use tari_ootle_common_types::displayable;
pub use tari_ootle_wallet_crypto as crypto;
pub use tari_template_lib_types as template_types;
pub use types::*;
