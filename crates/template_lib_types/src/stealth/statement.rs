//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::rust::{prelude::*, vec};

use super::{SpendWitness, StealthUnspentOutput};
use crate::{
    Amount,
    UtxoAddress,
    crypto::{BalanceProofSignature, PedersenCommitmentBytes, RangeProofBytes, RistrettoPublicKeyBytes},
};

/// A revealed output together with the key authorised to take it.
///
/// The revealed funds materialise as a bucket in the executing workspace rather than as a stealth output, so unlike a
/// confidential output nothing in the statement says where they end up. `receiver` supplies that: the engine requires
/// the receiver's badge in the transaction's auth scope before it creates the bucket, so a statement lifted into
/// another transaction yields its revealed funds to nobody. The receiver authorises the reveal; where the bucket goes
/// afterwards is up to the instructions that take it.
#[derive(Debug, Clone, Copy, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct RevealedOutput {
    /// The amount of revealed funds to output. Must be positive.
    #[n(0)]
    pub amount: Amount,
    /// The public key whose badge must be in the transaction's auth scope for the revealed bucket to be created.
    #[n(1)]
    pub receiver: RistrettoPublicKeyBytes,
}

impl RevealedOutput {
    pub fn new(amount: Amount, receiver: RistrettoPublicKeyBytes) -> Self {
        Self { amount, receiver }
    }
}

/// A statement for stealth outputs. A statement must contain confidential outputs
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct StealthOutputsStatement {
    /// The stealth outputs that are to be created
    #[n(0)]
    pub outputs: Vec<StealthUnspentOutput>,
    /// The revealed funds to output, and the key authorised to take them. When set, a bucket containing the revealed
    /// stealth funds is created for `receiver`. `None` is the only encoding of "no revealed output": a zero amount is
    /// rejected, so the presence of this field and the presence of a revealed bucket are the same thing.
    #[n(1)]
    pub revealed_output: Option<RevealedOutput>,
    /// Bulletproof range proof for the output commitments proving that values are in the range
    /// [minimum_value_promise, 2^64)
    #[n(2)]
    pub agg_range_proof: RangeProofBytes,
}

impl StealthOutputsStatement {
    /// Create a new output statement with no stealth outputs, only a revealed amount taken by `receiver`.
    pub fn new_revealed_only(amount: Amount, receiver: RistrettoPublicKeyBytes) -> Self {
        Self {
            outputs: vec![],
            revealed_output: Some(RevealedOutput::new(amount, receiver)),
            agg_range_proof: RangeProofBytes::empty(),
        }
    }

    /// The revealed output amount, or zero when there is no revealed output.
    pub fn revealed_output_amount(&self) -> Amount {
        self.revealed_output.map_or(Amount::ZERO, |r| r.amount)
    }

    /// The key authorised to take the revealed output, if there is one.
    pub fn revealed_receiver(&self) -> Option<RistrettoPublicKeyBytes> {
        self.revealed_output.map(|r| r.receiver)
    }
}

/// A statement for stealth outputs to spend as inputs.
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct StealthInput {
    /// The commitment of the unspent output being spent
    #[n(0)]
    pub commitment: PedersenCommitmentBytes,
    /// Selects which authorisation path the spender is exercising for this input (TIP-0006). Defaults to the key path.
    #[n(1)]
    #[cfg_attr(feature = "serde", serde(default))]
    #[cbor(default)]
    pub witness: SpendWitness,
}

impl StealthInput {
    /// A key-path spend of the output at `commitment`.
    pub fn new(commitment: PedersenCommitmentBytes) -> Self {
        Self {
            commitment,
            witness: SpendWitness::KeyPath,
        }
    }

    /// A spend of the output at `commitment` exercising `witness`.
    pub fn with_witness(commitment: PedersenCommitmentBytes, witness: SpendWitness) -> Self {
        Self { commitment, witness }
    }
}

impl From<&UtxoAddress> for StealthInput {
    fn from(address: &UtxoAddress) -> Self {
        address.id().into_commitment_bytes().into()
    }
}

impl From<PedersenCommitmentBytes> for StealthInput {
    fn from(commitment: PedersenCommitmentBytes) -> Self {
        Self::new(commitment)
    }
}
impl From<&PedersenCommitmentBytes> for StealthInput {
    fn from(commitment: &PedersenCommitmentBytes) -> Self {
        (*commitment).into()
    }
}

/// A statement for stealth outputs. A statement must contain confidential outputs
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct StealthInputsStatement {
    /// The stealth inputs that are to be spent
    #[n(0)]
    pub inputs: Vec<StealthInput>,
    /// The total amount of revealed funds being spent.
    #[n(1)]
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

/// A covenant sub-balance proof (TIP-0006 `AssertCovenantBalanced`) covering one partition of the transfer: the inputs
/// and outputs that share a `condition_root`. It proves the partition's committed value is conserved into outputs
/// carrying that root save for an exact cleartext `revealed_amount`, without exposing confidential values.
///
/// A claim is keyed by `partition_input_index` rather than by restating its condition root; the engine matches it to
/// its partition by that index. The partition's `condition_root` is bound into `signature`, so a claim cannot be
/// validated against the wrong partition.
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct CovenantBalanceClaim {
    /// The index, within the transfer's spent inputs, of the first input belonging to this partition. Identifies the
    /// partition without restating its spend condition.
    #[n(0)]
    pub partition_input_index: u32,
    /// The exact net cleartext amount leaving the partition (zero for full conservation). Must be non-negative.
    #[n(1)]
    pub revealed_amount: Amount,
    /// Schnorr proof of knowledge of the partition's aggregate mask difference.
    #[n(2)]
    pub signature: BalanceProofSignature,
}

#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct StealthTransferStatement {
    #[n(0)]
    pub inputs_statement: StealthInputsStatement,
    #[n(1)]
    pub outputs_statement: StealthOutputsStatement,
    /// Balance proof that proves that no coins were created or destroyed during the transfer (assuming the range proof
    /// is valid). This may be None, if and only if, the transfer is revealed-only (i.e. no stealth inputs or outputs).
    #[n(2)]
    pub balance_proof: Option<BalanceProofSignature>,
    /// Covenant sub-balance proofs (TIP-0006), one per input partition (keyed by `condition_root`) whose predicate
    /// requires value conservation. Empty when no spent input gates on a covenant.
    #[n(3)]
    #[cfg_attr(feature = "serde", serde(default))]
    #[cbor(default)]
    pub covenant_claims: Vec<CovenantBalanceClaim>,
}

impl StealthTransferStatement {
    pub fn revealed_only(input_amount: Amount, output_amount: Amount, receiver: RistrettoPublicKeyBytes) -> Self {
        Self {
            inputs_statement: StealthInputsStatement::new_revealed_only(input_amount),
            outputs_statement: StealthOutputsStatement::new_revealed_only(output_amount, receiver),
            balance_proof: None,
            covenant_claims: vec![],
        }
    }

    pub fn revealed_input_amount(&self) -> Amount {
        self.inputs_statement.revealed_amount
    }

    pub fn revealed_output_amount(&self) -> Amount {
        self.outputs_statement.revealed_output_amount()
    }

    /// The key authorised to take the revealed output, if there is one.
    pub fn revealed_receiver(&self) -> Option<RistrettoPublicKeyBytes> {
        self.outputs_statement.revealed_receiver()
    }

    pub fn stealth_outputs(&self) -> &[StealthUnspentOutput] {
        &self.outputs_statement.outputs
    }

    pub fn stealth_inputs(&self) -> &[StealthInput] {
        &self.inputs_statement.inputs
    }
}
