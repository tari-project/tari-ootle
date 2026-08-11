//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use borsh::{BorshSerialize, io};
use tari_crypto::ristretto::{RistrettoPublicKey, pedersen::PedersenCommitment};
use tari_template_lib::types::{
    Amount,
    Hash32,
    crypto::PedersenCommitmentBytes,
    stealth::{
        StealthInput,
        StealthInputsStatement,
        StealthOutputsStatement,
        StealthUnspentOutput,
        UnspentOutput,
        ViewableBalanceProofMessageFields,
    },
};

use crate::{
    Hash64,
    hashing::{EngineHashDomainLabel, engine_hasher64, hasher32},
};

pub fn confidential_withdraw64(
    excess: &RistrettoPublicKey,
    public_nonce: &RistrettoPublicKey,
    input_revealed_amount: &Amount,
    output_revealed_amount: &Amount,
) -> Hash64 {
    engine_hasher64(EngineHashDomainLabel::ConfidentialTransfer)
        .chain(excess)
        .chain(public_nonce)
        .chain(input_revealed_amount)
        .chain(output_revealed_amount)
        .result()
        .into()
}

pub fn viewable_balance_proof64(
    commitment: &PedersenCommitment,
    view_key: &RistrettoPublicKey,
    challenge_fields: ViewableBalanceProofMessageFields<'_>,
) -> Hash64 {
    engine_hasher64(EngineHashDomainLabel::ViewableBalanceProof)
        .chain(commitment)
        .chain(view_key)
        .chain(&challenge_fields)
        .result()
        .into()
}

/// Challenge for a stealth transfer's balance proof.
///
/// The preimage is split into [`StealthBalanceProofParts`] — the fields a verifier must hold verbatim to re-check
/// the proof — and a single [`stealth_balance_proof_aux32`] digest over the rest of the two statements. The halves
/// partition the statements, binding every field exactly once.
///
/// The split exists so that the challenge is reconstructible from a compact record via
/// [`stealth_balance_proof64_from_parts`], while the remainder — dominated by per-input witness data, bounded only
/// by `STEALTH_LIMITS.max_witness_data_len` each — collapses to 32 bytes.
///
/// The commitments are bound explicitly even though `public_excess` is a function of them: the excess is a lossy
/// sum, and binding the individual commitments prevents a signature being carried over to a different statement
/// that happens to share an excess.
pub fn stealth_balance_proof64(
    public_excess: &RistrettoPublicKey,
    public_nonce: &RistrettoPublicKey,
    stealth_inputs_statement: &StealthInputsStatement,
    stealth_outputs_statement: &StealthOutputsStatement,
) -> Hash64 {
    stealth_balance_proof64_from_parts(public_excess, public_nonce, &StealthBalanceProofParts {
        input_commitments: stealth_inputs_statement.inputs.iter().map(|i| &i.commitment),
        revealed_input_amount: &stealth_inputs_statement.revealed_amount,
        outputs: stealth_outputs_statement.outputs.iter().map(StealthOutputBinding::from),
        revealed_output_amount: &stealth_outputs_statement.revealed_output_amount,
        aux_digest: &stealth_balance_proof_aux32(stealth_inputs_statement, stealth_outputs_statement),
    })
}

/// [`stealth_balance_proof64`] reconstructed from a retained record rather than from the statements themselves.
///
/// This is the verification path for anyone re-checking a balance proof after the fact. `stealth_balance_proof64` is
/// this function with the parts derived from the live statements, so the two agree by construction.
pub fn stealth_balance_proof64_from_parts<'a, I, O>(
    public_excess: &RistrettoPublicKey,
    public_nonce: &RistrettoPublicKey,
    parts: &StealthBalanceProofParts<'a, I, O>,
) -> Hash64
where
    I: ExactSizeIterator<Item = &'a PedersenCommitmentBytes> + Clone,
    O: ExactSizeIterator<Item = StealthOutputBinding<'a>> + Clone,
{
    engine_hasher64(EngineHashDomainLabel::StealthBalanceProof)
        .chain(public_excess)
        .chain(public_nonce)
        .chain(parts)
        .result()
        .into()
}

/// Digest over everything in the two statements that [`StealthBalanceProofParts`] does not bind, so that between
/// them the two halves cover every field exactly once.
///
/// None of what this covers enters `public_excess`, so without it these fields would be malleable by anyone handling
/// an unsealed statement — rewriting an output's `auth` re-targets the funds while leaving the balance proof valid,
/// and rewriting `sender_public_nonce`/`encrypted_data` leaves the recipient unable to recover the output. The
/// sealed transaction covers the statement too, so this binding is what protects the pre-seal window in which
/// partial statements circulate during multi-party assembly.
///
/// Retaining this digest alongside the value fields is what makes a balance proof re-verifiable without the
/// statement: the fields it covers are unbounded in practice (per-input witness data, per-output encrypted data and
/// viewable-balance proofs, the aggregate range proof), and collapse here to 32 bytes.
pub fn stealth_balance_proof_aux32(
    stealth_inputs_statement: &StealthInputsStatement,
    stealth_outputs_statement: &StealthOutputsStatement,
) -> Hash32 {
    hasher32(EngineHashDomainLabel::StealthBalanceProofAux)
        .chain(&StealthBalanceProofAux {
            inputs: stealth_inputs_statement,
            outputs: stealth_outputs_statement,
        })
        .result()
}

struct StealthBalanceProofAux<'a> {
    inputs: &'a StealthInputsStatement,
    outputs: &'a StealthOutputsStatement,
}

impl BorshSerialize for StealthBalanceProofAux<'_> {
    fn serialize<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        // Every type is destructured exhaustively, so a field added to any of them stops this compiling rather than
        // silently escaping the balance proof's binding. The fields bound by `StealthBalanceProofParts` are the only
        // ones discarded here, which keeps the two halves of the preimage disjoint.
        let StealthInputsStatement {
            inputs,
            revealed_amount: _,
        } = self.inputs;
        let StealthOutputsStatement {
            outputs,
            revealed_output_amount: _,
            agg_range_proof,
        } = self.outputs;

        (inputs.len() as u32).serialize(writer)?;
        for StealthInput { commitment: _, witness } in inputs {
            witness.serialize(writer)?;
        }

        (outputs.len() as u32).serialize(writer)?;
        for StealthUnspentOutput { output, auth, tag } in outputs {
            let UnspentOutput {
                commitment: _,
                sender_public_nonce,
                encrypted_data,
                minimum_value_promise: _,
                viewable_balance_proof,
            } = output;

            sender_public_nonce.serialize(writer)?;
            encrypted_data.serialize(writer)?;
            viewable_balance_proof.serialize(writer)?;
            auth.serialize(writer)?;
            tag.serialize(writer)?;
        }

        agg_range_proof.serialize(writer)?;

        Ok(())
    }
}

/// Everything a verifier must retain to rebuild a stealth balance proof's challenge without the statement it was
/// signed over: the fields needed to recompute `public_excess` and to re-verify the aggregate range proof, plus the
/// [`stealth_balance_proof_aux32`] digest standing in for the rest of the statement.
///
/// The commitments are taken as iterators so that a caller holding the live statements can project them out without
/// materialising a list — the largest statements the engine admits carry a thousand inputs, and this is on the
/// consensus verification path.
pub struct StealthBalanceProofParts<'a, I, O> {
    pub input_commitments: I,
    pub revealed_input_amount: &'a Amount,
    pub outputs: O,
    pub revealed_output_amount: &'a Amount,
    pub aux_digest: &'a Hash32,
}

/// A stealth output as the balance proof binds it.
///
/// `minimum_value_promise` sits here rather than behind the aux digest because verifying the aggregate range proof
/// needs it verbatim, so any verifier working from a retained record holds it anyway. Binding it explicitly makes
/// the value that verifier uses the one the signer signed.
#[derive(Debug, Clone, Copy)]
pub struct StealthOutputBinding<'a> {
    pub commitment: &'a PedersenCommitmentBytes,
    pub minimum_value_promise: u64,
}

impl<'a> From<&'a StealthUnspentOutput> for StealthOutputBinding<'a> {
    fn from(output: &'a StealthUnspentOutput) -> Self {
        Self {
            commitment: output.commitment(),
            minimum_value_promise: output.output.minimum_value_promise,
        }
    }
}

impl<'a, I, O> BorshSerialize for StealthBalanceProofParts<'a, I, O>
where
    I: ExactSizeIterator<Item = &'a PedersenCommitmentBytes> + Clone,
    O: ExactSizeIterator<Item = StealthOutputBinding<'a>> + Clone,
{
    fn serialize<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        // Counts precede each sequence so that no two statements of differing shape share an encoding.
        (self.input_commitments.len() as u32).serialize(writer)?;
        for commitment in self.input_commitments.clone() {
            commitment.serialize(writer)?;
        }
        self.revealed_input_amount.serialize(writer)?;

        (self.outputs.len() as u32).serialize(writer)?;
        for output in self.outputs.clone() {
            output.commitment.serialize(writer)?;
            output.minimum_value_promise.serialize(writer)?;
        }
        self.revealed_output_amount.serialize(writer)?;

        self.aux_digest.serialize(writer)?;

        Ok(())
    }
}

/// Challenge for a covenant sub-balance proof (TIP-0006 `AssertCovenantBalanced`). Bound to a distinct domain from
/// [`stealth_balance_proof64`] so a partition proof can never be replayed as, or mistaken for, the whole-transfer
/// balance proof. The `condition_root`, `revealed_amount` and the ordered commitment lists pin the proof to one
/// partition.
pub fn covenant_balance_proof64(
    public_excess: &RistrettoPublicKey,
    public_nonce: &RistrettoPublicKey,
    condition_root: &Hash32,
    revealed_amount: &Amount,
    input_commitments: &[PedersenCommitmentBytes],
    output_commitments: &[PedersenCommitmentBytes],
) -> Hash64 {
    engine_hasher64(EngineHashDomainLabel::CovenantBalanceProof)
        .chain(public_excess)
        .chain(public_nonce)
        .chain(condition_root)
        .chain(revealed_amount)
        .chain(input_commitments)
        .chain(output_commitments)
        .result()
        .into()
}

pub fn value_proof_message(commitment: &PedersenCommitmentBytes, value: &Amount) -> Hash64 {
    engine_hasher64(EngineHashDomainLabel::ValueProof)
        .chain(commitment)
        .chain(value)
        .result()
        .into()
}
