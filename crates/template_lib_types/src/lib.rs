//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

// Support no_std environments
#![cfg_attr(not(feature = "std"), no_std)]

// This can be uncommented if you need to check for mistaken use of the std crate
// TODO: to always use this, we'd need to include the rust prelude where ever ts_rs is used.
// #![no_std]
// #[cfg(feature = "std")]
// extern crate std;

#[cfg(not(any(feature = "std", feature = "alloc")))]
compile_error!("Either feature `std` or `alloc` must be enabled for this crate.");
#[cfg(all(target_arch = "wasm32", feature = "std", feature = "alloc"))]
compile_error!("Feature `std` and `alloc` can't be enabled at the same time.");

#[macro_use]
mod amount;
#[macro_use]
pub mod crypto;
pub mod bytes;
pub mod constants;
mod encrypted_data;
pub mod engine_args;
mod entity_id;
mod error;
mod hash;
pub mod hex;
mod max_bytes;
mod max_string;
mod max_vec;
mod misc;
mod newtype_serde_macros;
mod resource_type;
pub mod serde_helpers;
#[macro_use]
mod substates;
pub mod access_rules;
pub mod address_prefixes;
mod auth_hook;
pub mod confidential;
mod owner_rule;
#[cfg(feature = "precision")]
pub mod precision;
pub mod stealth;

pub use access_rules::AccessRule;
pub use amount::*;
pub use auth_hook::*;
pub use encrypted_data::*;
pub use entity_id::*;
pub use error::*;
pub use hash::*;
pub use max_bytes::*;
pub use max_string::*;
pub use max_vec::*;
pub use misc::*;
pub use owner_rule::*;
pub use resource_type::*;
pub use substates::*;
