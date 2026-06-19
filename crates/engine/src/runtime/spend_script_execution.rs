//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::types::{
    Amount,
    Hash32,
    crypto::PedersenCommitmentBytes,
    stealth::{CovenantBalanceClaim, StealthInputView, StealthOutputView, StealthTransferStatement},
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
        }
    }
}
