//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::types::{
    Amount,
    crypto::PedersenCommitmentBytes,
    stealth::{
        CovenantBalanceClaim,
        SpendCondition,
        SpendScript,
        StealthInputView,
        StealthOutputView,
        StealthTransferStatement,
    },
};

/// The data a spend-script predicate can introspect, derived once from the spending `StealthTransferStatement` before
/// the predicate runs. Only commitments, spend conditions, `minimum_value_promise` and tags are exposed — never
/// confidential values.
#[derive(Debug, Clone)]
pub(crate) struct SpendScriptExecution {
    pub inputs: Vec<StealthInputView>,
    pub outputs: Vec<StealthOutputView>,
    /// The resolved spend condition of each input, parallel to `inputs`. Used to partition inputs by condition for
    /// covenant balance checks; not exposed to the predicate (only output conditions are visible).
    pub input_conditions: Vec<SpendCondition>,
    pub revealed_input_amount: Amount,
    pub revealed_output_amount: Amount,
    pub current_input_index: u32,
    pub current_input_commitment: PedersenCommitmentBytes,
    pub invoking_condition: SpendCondition,
    /// The covenant sub-balance proofs supplied by the spender, matched by partition index when the predicate invokes
    /// `AssertCovenantBalanced`.
    pub covenant_claims: Vec<CovenantBalanceClaim>,
}

impl SpendScriptExecution {
    /// Derives the introspection context for the predicate gating `input_index`. `input_conditions` holds the resolved
    /// spend condition of every input (parallel to the statement's inputs); `script` is the predicate being invoked.
    pub fn new(
        statement: &StealthTransferStatement,
        input_conditions: &[SpendCondition],
        input_index: u32,
        input_commitment: PedersenCommitmentBytes,
        script: &SpendScript,
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
                    spend_condition: o.spend_condition.clone(),
                    tag: o.tag,
                })
                .collect(),
            input_conditions: input_conditions.to_vec(),
            revealed_input_amount: statement.inputs_statement.revealed_amount,
            revealed_output_amount: statement.outputs_statement.revealed_output_amount,
            current_input_index: input_index,
            current_input_commitment: input_commitment,
            invoking_condition: SpendCondition::Script(script.clone()),
            covenant_claims: statement.covenant_claims.clone(),
        }
    }
}
