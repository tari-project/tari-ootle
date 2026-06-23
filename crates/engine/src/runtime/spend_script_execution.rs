//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::crypto::validate_covenant_balance_proof;
use tari_template_lib::types::{
    Amount,
    Hash32,
    crypto::PedersenCommitmentBytes,
    stealth::{
        CovenantBalanceClaim,
        StealthInputView,
        StealthOutputView,
        StealthTransferStatement,
        has_output_to,
        outputs_preserve_condition,
    },
};

/// The data a spend-script predicate can introspect, derived once from the spending `StealthTransferStatement` before
/// the predicate runs. Only commitments, the output authorisations (`spend_key`/`condition_root`),
/// `minimum_value_promise` and tags are exposed — never confidential values.
#[derive(Debug, Clone)]
pub(crate) struct SpendScriptExecution {
    pub inputs: Vec<StealthInputView>,
    pub outputs: Vec<StealthOutputView>,
    /// The committed `condition_root` of each input being spent via the script path, parallel to `inputs`; `None` for
    /// key-path inputs (which never participate in a covenant partition). Used to partition inputs by `condition_root`
    /// for covenant balance checks; not exposed to the predicate (only output roots are visible).
    pub input_condition_roots: Vec<Option<Hash32>>,
    pub revealed_input_amount: Amount,
    pub revealed_output_amount: Amount,
    pub current_input_index: u32,
    pub current_input_commitment: PedersenCommitmentBytes,
    /// The committed `condition_root` of the UTXO whose leaf is currently executing. Keys the covenant partition.
    pub current_input_condition_root: Hash32,
    /// The covenant sub-balance proofs supplied by the spender, matched by partition index when the predicate invokes
    /// `AssertCovenantBalanced`.
    pub covenant_claims: Vec<CovenantBalanceClaim>,
    /// The raw spender-supplied witness `data` blob for the invoking input, exposed to a `TemplateFunction` predicate
    /// via `SpendContext::data`.
    pub witness_data: Vec<u8>,
}

impl SpendScriptExecution {
    /// Derives the introspection context for the predicate gating `input_index`. `input_condition_roots` holds the
    /// committed root of every script-path input (parallel to the statement's inputs, `None` for key-path inputs);
    /// `current_input_condition_root` is the root of the UTXO whose leaf is being invoked.
    pub fn new(
        statement: &StealthTransferStatement,
        input_condition_roots: &[Option<Hash32>],
        input_index: u32,
        input_commitment: PedersenCommitmentBytes,
        current_input_condition_root: Hash32,
        witness_data: Vec<u8>,
    ) -> Self {
        Self {
            inputs: statement
                .inputs_statement
                .inputs
                .iter()
                .map(|i| StealthInputView {
                    commitment: i.commitment,
                })
                .collect(),
            outputs: statement
                .outputs_statement
                .outputs
                .iter()
                .map(|o| StealthOutputView {
                    commitment: o.output.commitment,
                    minimum_value_promise: o.output.minimum_value_promise,
                    auth: o.auth.clone(),
                    tag: o.tag,
                })
                .collect(),
            input_condition_roots: input_condition_roots.to_vec(),
            revealed_input_amount: statement.inputs_statement.revealed_amount,
            revealed_output_amount: statement.outputs_statement.revealed_output_amount,
            current_input_index: input_index,
            current_input_commitment: input_commitment,
            current_input_condition_root,
            covenant_claims: statement.covenant_claims.clone(),
            witness_data,
        }
    }

    /// "Stay in the vault": every stealth output is re-locked under exactly the invoking `condition_root` and nothing
    /// else, and there is at least one output. The authorisation must be `Script(root)` with no key path — a
    /// `KeyAndScript` output carrying the same root would be key-spendable next block, escaping the covenant, so it is
    /// not a preserving output. Bounds only the surviving outputs' authorisation, not the revealed amount.
    pub(crate) fn output_preserves_condition(&self) -> bool {
        outputs_preserve_condition(&self.outputs, &self.current_input_condition_root)
    }

    /// At least one stealth output is authorised by exactly `Script(condition_root)` (no key-path escape) and promises
    /// at least `min_value`.
    pub(crate) fn has_output_to(&self, condition_root: &Hash32, min_value: u64) -> bool {
        has_output_to(&self.outputs, condition_root, min_value)
    }

    /// Verifies the covenant sub-balance proof for the invoking partition (keyed by `current_input_condition_root`),
    /// returning whether its value is conserved up to a cleartext outflow of at most `max_revealed`.
    ///
    /// The partition is every input and output sharing that root. A claim is matched by the index of its partition's
    /// first input — no root is compared across the claim boundary; the proof signature binds the partition. A missing
    /// claim, an outflow over the allowance, or an invalid proof all yield `false`.
    pub(crate) fn covenant_balanced(&self, max_revealed: u64) -> bool {
        let me = self.current_input_condition_root;

        let Some(first_input_index) = self.input_condition_roots.iter().position(|root| *root == Some(me)) else {
            return false;
        };
        let Some(claim) = self
            .covenant_claims
            .iter()
            .find(|claim| claim.partition_input_index as usize == first_input_index)
        else {
            return false;
        };
        if claim.revealed_amount > Amount::from_u64(max_revealed) {
            return false;
        }

        let input_commitments = self
            .inputs
            .iter()
            .zip(&self.input_condition_roots)
            .filter(|(_, root)| **root == Some(me))
            .map(|(input, _)| input.commitment)
            .collect::<Vec<_>>();
        // Only outputs re-locked under exactly `Script(me)` stay in the partition; a `KeyAndScript` output committing
        // the same root carries a key-path escape, so its value is not conserved within the covenant (see
        // `StealthOutputView::is_locked_under`).
        let output_commitments = self
            .outputs
            .iter()
            .filter(|output| output.is_locked_under(&me))
            .map(|output| output.commitment)
            .collect::<Vec<_>>();

        validate_covenant_balance_proof(
            &me,
            claim.revealed_amount,
            &input_commitments,
            &output_commitments,
            &claim.signature,
        )
    }
}
