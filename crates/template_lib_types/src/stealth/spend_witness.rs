//    Copyright 2026 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::rust::prelude::*;

use super::SpendCondition;
use crate::Hash32;

/// Selects, per spent stealth input, which authorisation path the spender is exercising (TIP-0006).
///
/// The spend is witness-driven: the committed output only gates which witnesses are *admissible*
/// ([`StealthUnspentOutput::spend_key`](super::StealthUnspentOutput::spend_key) for the key path,
/// [`StealthUnspentOutput::condition_root`](super::StealthUnspentOutput::condition_root) for the script path); the
/// engine never assumes which path the spender picks. This type is pure data — no hashing — so it is safe to compile
/// into a template; the engine recomputes the Merkle root natively.
#[derive(Debug, Clone, Default, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub enum SpendWitness {
    /// Key-path spend: authorised by a signature from the output's `spend_key`, proven via the transaction's auth
    /// scope (a signer badge). Carries no payload — the signature lives in the transaction envelope.
    #[n(0)]
    #[default]
    KeyPath,
    /// Script-path spend: reveals one committed [`SpendCondition`] leaf and its inclusion proof against the output's
    /// `condition_root`. The engine recomputes the root, and on a match evaluates the leaf.
    #[n(1)]
    ScriptPath {
        #[n(0)]
        leaf: SpendCondition,
        #[n(1)]
        proof: MerkleProof,
    },
    // `#[n(2)]` is reserved for a future `ScriptPathTweaked` variant (the deferred taproot-style key tweak); do not
    // reuse this index.
}

impl SpendWitness {
    pub const fn is_key_path(&self) -> bool {
        matches!(self, Self::KeyPath)
    }

    pub const fn as_script_path(&self) -> Option<(&SpendCondition, &MerkleProof)> {
        match self {
            Self::ScriptPath { leaf, proof } => Some((leaf, proof)),
            _ => None,
        }
    }
}

/// An inclusion proof for a condition-tree (MAST) leaf: the sibling hashes from the leaf up to the root, bottom-first.
///
/// Carries no direction bits — the engine re-sorts each pair lexicographically on the way up. This type is pure data
/// (no hashing), so it lives beside the spend witness in the WASM-safe crate while the hashing stays native in
/// `tari_engine_types`.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, CborLen, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct MerkleProof {
    #[n(0)]
    pub siblings: Vec<Hash32>,
}

impl MerkleProof {
    pub fn new(siblings: Vec<Hash32>) -> Self {
        Self { siblings }
    }

    /// A proof for a single-leaf tree, where the leaf is itself the root.
    pub fn empty() -> Self {
        Self { siblings: Vec::new() }
    }
}
