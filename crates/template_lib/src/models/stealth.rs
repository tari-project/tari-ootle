//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_lib_types::{
    crypto::{
        BalanceProofSignature,
        PedersenCommitmentBytes,
        RangeProofBytes,
        RistrettoPublicKeyBytes,
        SchnorrSignatureBytes,
    },
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
    /// The signer that must sign the transaction to allow these inputs to be spent.
    pub required_signer: RistrettoPublicKeyBytes,
}

impl StealthInputsStatement {
    pub fn new(inputs: Vec<StealthInput>, revealed_amount: Amount, required_signer: RistrettoPublicKeyBytes) -> Self {
        assert!(!revealed_amount.is_negative(), "Revealed amount must be non-negative");
        assert!(
            !inputs.is_empty() || !revealed_amount.is_zero(),
            "At least one input or a revealed amount must be provided"
        );
        Self {
            inputs,
            revealed_amount,
            required_signer,
        }
    }

    /// Create a new input statement with no stealth inputs, only a revealed amount.
    pub fn new_revealed_only(amount: Amount, required_signer: RistrettoPublicKeyBytes) -> Self {
        Self::new(vec![], amount, required_signer)
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
    #[cfg_attr(feature = "ts", ts(type = "{public_nonce: string, signature: string} | null"))]
    pub balance_proof: Option<BalanceProofSignature>,
}

impl StealthTransferStatement {
    pub fn revealed_only(
        input_amount: Amount,
        output_amount: Amount,
        required_signer: RistrettoPublicKeyBytes,
    ) -> Self {
        Self {
            inputs_statement: StealthInputsStatement::new_revealed_only(input_amount, required_signer),
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
}
