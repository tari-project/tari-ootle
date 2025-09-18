//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_lib_types::{
    crypto::{BalanceProofSignature, PedersenCommitmentBytes, RangeProofBytes, SchnorrSignatureBytes},
    Amount,
};

use crate::models::StealthUnspentOutput;

/// A statement for stealth outputs. A statement must contain confidential outputs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct StealthOutputsStatement {
    /// The stealth outputs that are to be created
    pub outputs: Vec<StealthUnspentOutput>,
    /// The amount of revealed funds to output. If this is a positive (non-zero) value, a bucket containing the
    /// revealed stealth funds is created.
    pub revealed_output_amount: Amount,
    /// Bulletproof range proof for the output commitments proving that values are in the range
    /// [minimum_value_promise, 2^64)
    // TODO: consider creating multiple batches of outputs each with an aggregate BP, since BP+ initialization for
    // arbitrary number (tested 512) is expensive and slow
    pub agg_range_proof: RangeProofBytes,
}

/// A statement for stealth outputs. A statement must contain confidential outputs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct StealthInput {
    /// The commitment of the unspent output being spent
    pub commitment: PedersenCommitmentBytes,
    /// Signature that proves ownership of the unspent output. This must be signed by the owner_public_key of the
    /// output.
    pub owner_proof: SchnorrSignatureBytes,
}
/// A statement for stealth outputs. A statement must contain confidential outputs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct StealthInputsStatement {
    /// The stealth inputs that are to be spent
    pub inputs: Vec<StealthInput>,
    /// The total amount of revealed funds being spent.
    pub revealed_amount: Amount,
}

impl StealthInputsStatement {
    pub fn new(inputs: Vec<StealthInput>, revealed_amount: Amount) -> Self {
        assert!(!revealed_amount.is_negative(), "Revealed amount must be positive");
        assert!(
            !inputs.is_empty() || !revealed_amount.is_zero(),
            "At least one input or a revealed amount must be provided"
        );
        Self {
            inputs,
            revealed_amount,
        }
    }

    pub fn new_revealed(amount: Amount) -> Self {
        Self::new(vec![], amount)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct StealthTransferStatement {
    pub inputs_statement: StealthInputsStatement,
    pub outputs_statement: StealthOutputsStatement,
    /// Balance proof that proves that no coins were created or destroyed during the transfer (assuming the range proof
    /// is valid).
    #[cfg_attr(feature = "ts", ts(type = "{public_nonce: string, signature: string}"))]
    pub balance_proof: BalanceProofSignature,
}

impl StealthTransferStatement {
    pub fn revealed_input_amount(&self) -> Amount {
        self.inputs_statement.revealed_amount
    }

    pub fn revealed_output_amount(&self) -> Amount {
        self.outputs_statement.revealed_output_amount
    }
}
