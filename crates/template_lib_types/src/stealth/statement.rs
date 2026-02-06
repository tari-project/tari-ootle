//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_abi::rust::{prelude::*, vec};

use super::StealthUnspentOutput;
use crate::{
    Amount,
    UtxoAddress,
    crypto::{BalanceProofSignature, PedersenCommitmentBytes, RangeProofBytes},
};

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
    pub agg_range_proof: RangeProofBytes,
}

impl StealthOutputsStatement {
    /// Create a new output statement with no stealth outputs, only a revealed amount.
    pub fn new_revealed_only(amount: Amount) -> Self {
        Self {
            outputs: vec![],
            revealed_output_amount: amount,
            agg_range_proof: RangeProofBytes::empty(),
        }
    }
}

/// A statement for stealth outputs to spend as inputs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct StealthInput {
    /// The commitment of the unspent output being spent
    pub commitment: PedersenCommitmentBytes,
}

impl StealthInput {
    pub fn new(commitment: PedersenCommitmentBytes) -> Self {
        Self { commitment }
    }
}

impl From<&UtxoAddress> for StealthInput {
    fn from(address: &UtxoAddress) -> Self {
        address.id().into_commitment_bytes().into()
    }
}

impl From<PedersenCommitmentBytes> for StealthInput {
    fn from(commitment: PedersenCommitmentBytes) -> Self {
        Self { commitment }
    }
}
impl From<&PedersenCommitmentBytes> for StealthInput {
    fn from(commitment: &PedersenCommitmentBytes) -> Self {
        (*commitment).into()
    }
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
        assert!(!revealed_amount.is_negative(), "Revealed amount must be non-negative");
        assert!(
            !inputs.is_empty() || !revealed_amount.is_zero(),
            "At least one input or a revealed amount must be provided"
        );
        Self {
            inputs,
            revealed_amount,
        }
    }

    /// Create a new input statement with no stealth inputs, only a revealed amount.
    pub fn new_revealed_only(amount: Amount) -> Self {
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
    /// is valid). This may be None, if and only if, the transfer is revealed-only (i.e. no stealth inputs or outputs).
    pub balance_proof: Option<BalanceProofSignature>,
}

impl StealthTransferStatement {
    pub fn revealed_only(input_amount: Amount, output_amount: Amount) -> Self {
        Self {
            inputs_statement: StealthInputsStatement::new_revealed_only(input_amount),
            outputs_statement: StealthOutputsStatement::new_revealed_only(output_amount),
            balance_proof: None,
        }
    }

    pub fn revealed_input_amount(&self) -> Amount {
        self.inputs_statement.revealed_amount
    }

    pub fn revealed_output_amount(&self) -> Amount {
        self.outputs_statement.revealed_output_amount
    }

    pub fn stealth_outputs(&self) -> &[StealthUnspentOutput] {
        &self.outputs_statement.outputs
    }

    pub fn stealth_inputs(&self) -> &[StealthInput] {
        &self.inputs_statement.inputs
    }
}
