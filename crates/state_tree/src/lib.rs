//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod error;
pub use error::*;

pub mod key_mapper;
pub mod memory_store;

mod staged_store;
pub use staged_store::*;

mod traits;
pub use traits::*;

mod diff;
pub use diff::*;
pub mod empty_store;
mod hasher;
mod helpers;
mod tree;

pub use jmt::{storage, KeyHash, OwnedValue, RootHash, Version};
pub use tree::*;

use crate::hasher::OotleJmtHasher;

/// The payload type used in the state tree. This is a reference to a particular substate (i.e. SubstateAddress).
pub type StateTreePayload = tari_ootle_common_types::SubstateAddress;

pub type SparseMerkleProof = jmt::proof::SparseMerkleProof<OotleJmtHasher>;
