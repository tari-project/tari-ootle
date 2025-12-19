//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_bor::{Deserialize, Serialize};
use tari_template_lib_types::{
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, UtxoTag},
    EncryptedData,
};

use crate::{auth::AccessRule, models::ViewableBalanceProof};

/// An unspent output that does not reveal the value and the owner of the coin it represents.
///
/// Unspent outputs contain:
/// - **commitment** - the Pedersen commitment k.G + v.H
/// - **sender_public_nonce** - the sender-provided public nonce that is used as part of a DH key exchange to generate
///   the decryption key for the encrypted data.
/// - **encrypted_data** - the encrypted data that contains the encrypted mask and value.
/// - **viewable_balance_proof** - an optional verifiable balance proof that must be provided and valid if the view key
///   is enabled for a resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct UnspentOutput {
    pub commitment: PedersenCommitmentBytes,
    /// Public nonce (R) that was used to generate the commitment mask
    pub sender_public_nonce: RistrettoPublicKeyBytes,
    /// Encrypted mask and value for the recipient.
    pub encrypted_data: EncryptedData,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub minimum_value_promise: u64,
    /// If the view key is enabled for a given resource, this proof MUST be provided, otherwise it MUST NOT.
    pub viewable_balance_proof: Option<ViewableBalanceProof>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct StealthUnspentOutput {
    pub output: UnspentOutput,
    pub spend_condition: SpendCondition,
    pub tag: UtxoTag,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum SpendCondition {
    /// The public key that must prove ownership of this UTXO. This is typically a one time "stealth" public key but is
    /// selected by the client.
    Signed(RistrettoPublicKeyBytes),
    AccessRule(AccessRule),
}

impl SpendCondition {
    pub const fn signed_by(&self) -> Option<&RistrettoPublicKeyBytes> {
        match self {
            Self::Signed(pk) => Some(pk),
            _ => None,
        }
    }

    pub const fn as_type_str(&self) -> &'static str {
        match self {
            Self::Signed(_) => "SignedBy",
            Self::AccessRule(_) => "AccessRule",
        }
    }
}
