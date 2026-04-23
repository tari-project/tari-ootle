//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! # Consensus directives
//!
//! Operator-signed messages used for break-glass recovery on a running Ootle sidechain.
//!
//! A [`ConsensusDirective`] is delivered out-of-band to validator nodes (via an admin RPC)
//! when normal consensus cannot make progress — for example to roll a validator back to a
//! prior epoch checkpoint after a consensus fault. Directives are authenticated by a
//! configured governance public key and are idempotent: validators persist applied directive
//! IDs so repeated delivery of the same directive is a no-op.
//!
//! ## Cryptography
//!
//! - Body is serialised with borsh for canonical bytes.
//! - [`DirectiveId`] is Blake2b-256 of the domain-tagged canonical bytes.
//! - Signatures are RistrettoSchnorr over the [`DirectiveId`].

mod body;
#[allow(clippy::module_inception)]
mod directive;
mod signature;

pub use body::{DirectiveBody, DirectiveKind};
pub use directive::{ConsensusDirective, DirectiveError, DirectiveId};
pub use signature::DirectiveSignature;
