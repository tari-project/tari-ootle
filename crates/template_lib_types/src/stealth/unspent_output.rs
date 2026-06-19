//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::rust::prelude::*;

use super::ViewableBalanceProof;
use crate::{
    EncryptedData,
    FunctionName,
    Hash32,
    TemplateAddress,
    access_rules::AccessRule,
    bytes::Bytes,
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, UtxoTag},
};

/// An unspent output that does not reveal the value and the owner of the coin it represents.
///
/// Unspent outputs contain:
/// - **commitment** - the Pedersen commitment k.G + v.H
/// - **sender_public_nonce** - the sender-provided public nonce that is used as part of a DH key exchange to generate
///   the decryption key for the encrypted data.
/// - **encrypted_data** - the encrypted data that contains the encrypted mask and value.
/// - **viewable_balance_proof** - an optional verifiable balance proof that must be provided and valid if the view key
///   is enabled for a resource.
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct UnspentOutput {
    #[n(0)]
    pub commitment: PedersenCommitmentBytes,
    /// Public nonce (R) that was used to generate the commitment mask
    #[n(1)]
    pub sender_public_nonce: RistrettoPublicKeyBytes,
    /// Encrypted mask and value for the recipient.
    #[n(2)]
    pub encrypted_data: EncryptedData,
    #[n(3)]
    pub minimum_value_promise: u64,
    /// If the view key is enabled for a given resource, this proof MUST be provided, otherwise it MUST NOT.
    #[n(4)]
    pub viewable_balance_proof: Option<ViewableBalanceProof>,
}

/// How a stealth output is authorised at spend time (TIP-0006): a key path, a committed condition tree, or both.
///
/// Modelled as an enum rather than a pair of `Option`s so the unspendable `{no key, no conditions}` state is
/// unrepresentable — by construction in memory and at the decode boundary, with no runtime invariant to enforce.
///
/// - **Key** — a one-time "stealth" public key; spendable by proving ownership of it (a signature, via the
///   transaction's auth scope).
/// - **Script** — the Merkle root (MAST) committing the set of alternative [`SpendCondition`] leaves; spendable by
///   revealing one leaf plus an inclusion proof.
/// - **KeyAndScript** — either path is admissible; the per-input `SpendWitness` selects which.
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub enum SpendAuthorization {
    #[n(0)]
    Key(#[n(0)] RistrettoPublicKeyBytes),
    #[n(1)]
    Script(#[n(0)] Hash32),
    #[n(2)]
    KeyAndScript {
        #[n(0)]
        spend_key: RistrettoPublicKeyBytes,
        #[n(1)]
        condition_root: Hash32,
    },
}

impl SpendAuthorization {
    /// The one-time key authorising a key-path spend, if this output has a key path.
    pub fn spend_key(&self) -> Option<RistrettoPublicKeyBytes> {
        match self {
            Self::Key(key) | Self::KeyAndScript { spend_key: key, .. } => Some(*key),
            Self::Script(_) => None,
        }
    }

    /// The committed condition-tree root authorising a script-path spend, if this output has a condition tree.
    pub fn condition_root(&self) -> Option<Hash32> {
        match self {
            Self::Script(root) |
            Self::KeyAndScript {
                condition_root: root, ..
            } => Some(*root),
            Self::Key(_) => None,
        }
    }
}

/// A stealth unspent output, authorised at spend time per its [`SpendAuthorization`] (TIP-0006).
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct StealthUnspentOutput {
    #[n(0)]
    pub output: UnspentOutput,
    #[n(1)]
    pub auth: SpendAuthorization,
    #[n(2)]
    pub tag: UtxoTag,
}

impl StealthUnspentOutput {
    pub fn commitment(&self) -> &PedersenCommitmentBytes {
        &self.output.commitment
    }
}

/// A spend condition leaf (v0) committed in a [`StealthUnspentOutput::condition_root`] tree. A script-path spend
/// reveals one leaf and an inclusion proof; the engine recomputes the root and, on a match, evaluates the leaf.
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum SpendCondition {
    /// Spend is gated on a native access rule evaluated against the transaction's auth scope.
    #[n(0)]
    AccessRule(#[n(0)] AccessRule),
    /// Spend is gated on a stateless WASM predicate over the spending transfer. The referenced template function
    /// introspects the spending `StealthTransferStatement` and rejects the spend by panicking.
    #[n(1)]
    TemplateFunction(#[n(0)] TemplateFunction),
}

impl SpendCondition {
    pub const fn as_template_function(&self) -> Option<&TemplateFunction> {
        match self {
            Self::TemplateFunction(func) => Some(func),
            _ => None,
        }
    }
}

/// A stateless WASM predicate that gates the spend of a stealth UTXO.
///
/// The condition fully commits to `{template, function, args}`, so a spender cannot substitute a different predicate.
/// Templates are immutable substates, so the referenced code cannot change once the output is committed.
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct TemplateFunction {
    /// The template providing the predicate.
    #[n(0)]
    pub template: TemplateAddress,
    /// The stateless (`is_mut == false`) predicate function on that template.
    #[n(1)]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub function: FunctionName,
    /// Bound parameters, positional — one CBOR-encoded value per leading (non-`SpendContext`) parameter, matching the
    /// engine's `Vec<Bytes>` call ABI. Baked into the condition at creation; the spender cannot alter them.
    #[n(2)]
    pub args: Vec<Bytes>,
}

impl TemplateFunction {
    pub fn new(template: TemplateAddress, function: FunctionName, args: Vec<Bytes>) -> Self {
        Self {
            template,
            function,
            args,
        }
    }
}
