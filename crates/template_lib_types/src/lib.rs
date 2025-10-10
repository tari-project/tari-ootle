//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[macro_use]
mod amount;
#[macro_use]
pub mod crypto;
pub mod bytes;
mod encrypted_data;
pub mod engine_args;
mod entity_id;
mod error;
mod hash;
pub mod hex;
mod max_bytes;
mod max_string;
mod resource_type;
pub mod serde_helpers;

pub use amount::*;
pub use encrypted_data::*;
pub use entity_id::*;
pub use error::*;
pub use hash::*;
pub use max_bytes::MaxBytes;
pub use max_string::MaxString;
pub use resource_type::*;

/// The address of a Template
pub type TemplateAddress = Hash;
