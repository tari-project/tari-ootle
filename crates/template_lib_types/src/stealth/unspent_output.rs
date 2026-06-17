//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::rust::prelude::*;

use super::ViewableBalanceProof;
use crate::{
    EncryptedData,
    FunctionName,
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

#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct StealthUnspentOutput {
    #[n(0)]
    pub output: UnspentOutput,
    #[n(1)]
    pub spend_condition: SpendCondition,
    #[n(2)]
    pub tag: UtxoTag,
}

impl StealthUnspentOutput {
    pub fn commitment(&self) -> &PedersenCommitmentBytes {
        &self.output.commitment
    }
}

#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum SpendCondition {
    /// The public key that must prove ownership of this UTXO. This is typically a one time "stealth" public key but is
    /// selected by the client.
    #[n(0)]
    Signed(#[n(0)] RistrettoPublicKeyBytes),
    #[n(1)]
    AccessRule(#[n(0)] AccessRule),
    /// Spend is gated on a stateless WASM predicate over the spending transfer. The referenced template function
    /// introspects the spending `StealthTransferStatement` and rejects the spend by panicking.
    #[n(2)]
    Script(#[n(0)] SpendScript),
}

impl SpendCondition {
    pub const fn signed_by(&self) -> Option<&RistrettoPublicKeyBytes> {
        match self {
            Self::Signed(pk) => Some(pk),
            _ => None,
        }
    }

    pub const fn as_script(&self) -> Option<&SpendScript> {
        match self {
            Self::Script(script) => Some(script),
            _ => None,
        }
    }

    pub const fn as_type_str(&self) -> &'static str {
        match self {
            Self::Signed(_) => "SignedBy",
            Self::AccessRule(_) => "AccessRule",
            Self::Script(_) => "Script",
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
pub struct SpendScript {
    /// The template providing the predicate.
    #[n(0)]
    pub template: TemplateAddress,
    /// The stateless (`is_mut == false`) predicate function on that template.
    #[n(1)]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub function: FunctionName,
    /// Bound parameters, positional — one CBOR-encoded value per leading (non-`SpendContext`) parameter, matching the
    /// engine's `Vec<Bytes>` call ABI. Baked into the output at creation; the spender cannot alter them.
    #[n(2)]
    pub args: Vec<Bytes>,
}

impl SpendScript {
    pub fn new(template: TemplateAddress, function: FunctionName, args: Vec<Bytes>) -> Self {
        Self {
            template,
            function,
            args,
        }
    }
}
