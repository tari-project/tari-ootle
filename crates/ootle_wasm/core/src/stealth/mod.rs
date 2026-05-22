//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Pure-Rust stealth crypto primitives exposed to WASM.
//!
//! Each submodule wraps a slice of the daemon-side stealth flow so wallets can build stealth transfers
//! entirely client-side. The thin `#[wasm_bindgen]` bindings live in the sibling `ootle-wasm` crate.

pub mod balance_proof;
pub mod encrypted_data;
pub mod inputs;
pub mod kdfs;
pub mod outputs;
mod types;
pub mod validate;
pub mod viewable_balance;

pub use types::{InputWitness, OutputWitnessJson, StealthInputWitnessJson, StealthOutputWitnessJson};
