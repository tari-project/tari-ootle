//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod crypto;
mod entity_id;
mod error;
mod hash;
pub mod serde_helpers;

pub use entity_id::*;
pub use error::*;
pub use hash::*;

/// The address of a Template
pub type TemplateAddress = Hash;
