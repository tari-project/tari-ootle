//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_lib_types::{
    crypto::{BalanceProofSignature, PedersenCommitmentBytes, RangeProofBytes},
    Amount,
};

use crate::models::UnspentOutput;

/// A statement for stealth outputs. A statement must contain confidential outputs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct StealthOutputsStatement {
    /// Proof of the confidential resources that are going to be transferred to the receiver
    pub outputs: Vec<UnspentOutput>,
    /// The amount of revealed funds to output. If this is a positive (non-zero) value, a bucket containing the
    /// revealed stealth funds is created.
    pub revealed_output_amount: Amount,
    /// Bulletproof range proof for the output commitments proving that values are in the range
    /// [minimum_value_promise, 2^64)
    pub agg_range_proof: RangeProofBytes,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct StealthMintBalanceProof {
    pub total_mint_amount: Amount,
    #[cfg_attr(feature = "ts", ts(type = "{public_nonce: string, signature: string}"))]
    pub excess_signature: BalanceProofSignature,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct StealthMintStatement {
    pub balance_proof: StealthMintBalanceProof,
    pub outputs_statement: StealthOutputsStatement,
}

impl StealthMintStatement {
    pub fn unverified_total_amount(&self) -> Amount {
        self.balance_proof.total_mint_amount
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct StealthTransferStatement {
    pub inputs: Vec<PedersenCommitmentBytes>,
    pub outputs_statement: StealthOutputsStatement,
    /// Balance proof that proves that no coins were created or destroyed during the transfer (assuming the range proof
    /// is valid).
    #[cfg_attr(feature = "ts", ts(type = "{public_nonce: string, signature: string}"))]
    pub balance_proof: BalanceProofSignature,
}
