//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};

use crate::{
    Amount,
    crypto::{BalanceProofSignature, PedersenCommitmentBytes, RangeProofBytes},
    stealth::UnspentOutput,
};

/// A statement for confidential and revealed outputs. A statement must contain either confidential outputs or non-zero
/// revealed funds or both.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ConfidentialOutputStatement {
    /// Output that is transferred to the receiver account
    pub output: Option<UnspentOutput>,
    /// Change output that goes back to the sender's vault
    pub change_statement: Option<UnspentOutput>,
    /// Bulletproof range proof for the output and change commitments proving that values are in the range
    /// [minimum_value_promise, 2^64)
    pub range_proof: RangeProofBytes,
    /// The amount of revealed funds to output
    pub output_revealed_amount: Amount,
    /// The amount of revealed funds to return to the sender
    pub change_revealed_amount: Amount,
}

impl ConfidentialOutputStatement {
    /// Creates an output proof for minting which only mints a revealed amount.
    pub fn mint_revealed<T: Into<Amount>>(amount: T) -> Self {
        Self {
            output: None,
            change_statement: None,
            range_proof: RangeProofBytes::empty(),
            output_revealed_amount: amount.into(),
            change_revealed_amount: Amount::zero(),
        }
    }
}

/// A confidential proof that defines a confidential and/or revealed withdrawal, e.g. from a vault containing
/// confidential resources. This proof contains:
/// - Inputs: The confidential inputs that are being withdrawn identified by their Pedersen commitments.
/// - Input revealed amount: The amount of revealed funds to withdraw.
/// - Output proof: The confidential output statement that contains the output and change statements, range proof, and
///   revealed amounts.
/// - Balance proof: The balance proof signature that proves knowledge of the excess. inputs - output - change = (0)
///   where (0) is the excess. Knowledge of the excess is not possible unless the inputs and outputs balance.
///
/// Withdrawals can be revealed only, confidential only, or a mix of both.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ConfidentialWithdrawProof {
    pub inputs: Vec<PedersenCommitmentBytes>,
    /// The amount to withdraw from revealed funds i.e. the revealed funds as inputs
    pub input_revealed_amount: Amount,
    pub output_proof: ConfidentialOutputStatement,
    /// Balance proof
    pub balance_proof: BalanceProofSignature,
}

impl ConfidentialWithdrawProof {
    /// Creates a valid withdrawal proof for revealed funds of a specific amount.
    pub fn revealed_withdraw<T: Into<Amount>>(amount: T) -> Self {
        // There are no confidential inputs or outputs (this amounts to the same thing as a Fungible resource transfer)
        // So signature s = 0 + e.x where x is a 0 excess, is technically valid. Note that signature verification
        // explicitly disallows the zero key. However, we explicitly check for this case in the
        // `is_revealed_only` method and consider the signature valid.
        let balance_proof = BalanceProofSignature::zero();

        let amount = amount.into();
        Self {
            inputs: vec![],
            input_revealed_amount: amount,
            output_proof: ConfidentialOutputStatement::mint_revealed(amount),
            balance_proof,
        }
    }

    /// Creates a withdrawal proof for a confidential transfer that transfers revealed funds to confidential outputs.
    pub fn revealed_to_confidential<T: Into<Amount>>(
        input_revealed_amount: T,
        output_proof: ConfidentialOutputStatement,
        balance_proof: BalanceProofSignature,
    ) -> Self {
        Self {
            inputs: vec![],
            input_revealed_amount: input_revealed_amount.into(),
            output_proof,
            balance_proof,
        }
    }

    /// Returns true if the withdraw proof is only transferring revealed funds, otherwise false
    /// The method for determining this is strict, as this can be used to determine whether to
    /// safely skip the balance proof check. To return true it requires:
    /// - Empty inputs
    /// - Output and Change outputs must be None
    /// - Empty range proof
    /// - Zero balance proof
    /// - Revealed funds > 0 in the inputs and outputs
    pub fn is_revealed_only(&self) -> bool {
        // Range proof must be empty
        self.output_proof.range_proof.is_empty() &&
        // Excess will be zero
        self.inputs.is_empty() &&
            self.output_proof.output.is_none() &&
            self.output_proof.change_statement.is_none() &&
            // zero balance proof
            self.balance_proof == BalanceProofSignature::zero() &&
            // There are revealed funds
            self.input_revealed_amount > Amount::zero() &&
            self.output_proof.output_revealed_amount.checked_add(self.output_proof.change_revealed_amount).is_some_and(|a| a > Amount::zero())
    }

    pub fn revealed_input_amount(&self) -> Amount {
        self.input_revealed_amount
    }

    pub fn revealed_output_amount(&self) -> Amount {
        self.output_proof.output_revealed_amount
    }

    pub fn revealed_change_amount(&self) -> Amount {
        self.output_proof.change_revealed_amount
    }
}
