//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Confidential (stealth) transfer support.
//!
//! [`StealthTransfer`] builds stealth transfer statements with Pedersen commitments,
//! encrypted memos, and change outputs. It handles both revealed inputs (e.g. from
//! the faucet) and stealth inputs with spending proofs.

mod builder;
mod error;
mod spec;
mod traits;

pub use builder::*;
pub use error::*;
pub use spec::*;
pub use traits::*;
