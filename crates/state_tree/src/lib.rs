//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub use tari_jellyfish::*;

mod error;
pub use error::*;

pub mod key_mapper;
pub mod memory_store;

mod staged_store;
pub use staged_store::*;

mod traits;
pub use traits::*;

mod tree;

pub use tree::*;

/// The payload type used in the state tree. This is a reference to a particular substate (i.e. SubstateAddress).
pub type StateTreePayload = tari_ootle_common_types::SubstateAddress;
