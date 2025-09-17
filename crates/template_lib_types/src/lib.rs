//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[macro_use]
mod amount;
#[macro_use]
pub mod crypto;
pub mod engine_args;
mod entity_id;
mod error;
mod hash;
pub mod hex;
pub mod serde_helpers;

pub use amount::*;
pub use entity_id::*;
pub use error::*;
pub use hash::*;

/// The address of a Template
pub type TemplateAddress = Hash;
