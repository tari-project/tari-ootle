//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[macro_use]
mod amount;
pub mod crypto;
mod entity_id;
mod error;
mod hash;
mod hex;
pub mod serde_helpers;

pub use amount::*;
pub use entity_id::*;
pub use error::*;
pub use hash::*;

/// The address of a Template
pub type TemplateAddress = Hash;
