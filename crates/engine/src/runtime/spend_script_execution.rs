//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use tari_engine_types::crypto::{covenant_balance_proof_native_points, validate_covenant_balance_proof};
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

use crate::runtime::RuntimeError;

/// The data a spend-script predicate can introspect about the spending `StealthTransferStatement`, derived once per
/// statement and shared by every predicate gating one of its inputs. Only commitments, the output authorisations
/// (`spend_key`/`condition_root`), `minimum_value_promise` and tags are exposed — never confidential values.
#[derive(Debug)]
pub(crate) struct StatementSpendView {
    pub inputs: Vec<StealthInputView>,
    pub outputs: Vec<StealthOutputView>,
    /// The committed `condition_root` of each input being spent via the script path, parallel to `inputs`; `None` for
    /// key-path inputs (which never participate in a covenant partition). Used to partition inputs by `condition_root`
    /// for covenant balance checks; not exposed to the predicate (only output roots are visible).
    pub input_condition_roots: Vec<Option<Hash32>>,
    pub revealed_input_amount: Amount,
    pub revealed_output_amount: Amount,
    /// The covenant sub-balance proofs supplied by the spender, matched by partition index.
    covenant_claims: Vec<CovenantBalanceClaim>,
    /// The outcome of each partition's sub-balance proof, keyed by the partition's `condition_root`: the proven
    /// cleartext outflow, or `None` when the claim is missing or its proof is invalid. Every covenant atom of every
    /// input in a partition asks about the same proof, so it is verified (and charged) once per statement.
    verified_partitions: RefCell<HashMap<Hash32, Option<Amount>>>,
}

impl StatementSpendView {
    /// `input_condition_roots` holds the committed root of every script-path input, parallel to the statement's inputs
    /// (`None` for key-path inputs).
    pub fn new(statement: &StealthTransferStatement, input_condition_roots: Vec<Option<Hash32>>) -> Self {
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
            input_condition_roots,
            revealed_input_amount: statement.inputs_statement.revealed_amount,
            revealed_output_amount: statement.outputs_statement.revealed_output_amount(),
            covenant_claims: statement.covenant_claims.clone(),
            verified_partitions: RefCell::new(HashMap::new()),
        }
    }

    /// "Stay in the vault": every stealth output is re-locked under exactly `condition_root` and nothing else, and
    /// there is at least one output. The authorisation must be `Script(root)` with no key path — a `KeyAndScript`
    /// output carrying the same root would be key-spendable next block, escaping the covenant, so it is not a
    /// preserving output. Bounds only the surviving outputs' authorisation, not the revealed amount.
    pub fn output_preserves_condition(&self, condition_root: &Hash32) -> bool {
        outputs_preserve_condition(&self.outputs, condition_root)
    }

    /// At least one stealth output is authorised by exactly `Script(condition_root)` (no key-path escape) and promises
    /// at least `min_value`.
    pub fn has_output_to(&self, condition_root: &Hash32, min_value: u64) -> bool {
        has_output_to(&self.outputs, condition_root, min_value)
    }

    /// Whether the partition keyed by `condition_root` conserves value up to a cleartext outflow of at most
    /// `max_revealed`, per its covenant sub-balance proof.
    ///
    /// The partition is every input and output sharing that root. A claim is matched by the index of its partition's
    /// first input — no root is compared across the claim boundary; the proof signature binds the partition. A missing
    /// claim, an outflow over the allowance, or an invalid proof all yield `false`.
    ///
    /// The proof's native verification work is passed to `charge` before it runs, on the partition's first query only.
    pub fn covenant_balanced<F>(
        &self,
        condition_root: &Hash32,
        max_revealed: u64,
        charge: F,
    ) -> Result<bool, RuntimeError>
    where
        F: FnOnce(u64) -> Result<(), RuntimeError>,
    {
        let cached = self.verified_partitions.borrow().get(condition_root).copied();
        let proven_outflow = match cached {
            Some(outcome) => outcome,
            None => {
                let outcome = self.verify_partition(condition_root, charge)?;
                self.verified_partitions.borrow_mut().insert(*condition_root, outcome);
                outcome
            },
        };
        Ok(proven_outflow.is_some_and(|outflow| outflow <= Amount::from_u64(max_revealed)))
    }

    /// Verifies the sub-balance proof of the partition keyed by `condition_root`, returning its proven cleartext
    /// outflow, or `None` if the partition has no claim or the proof is invalid.
    fn verify_partition<F>(&self, condition_root: &Hash32, charge: F) -> Result<Option<Amount>, RuntimeError>
    where F: FnOnce(u64) -> Result<(), RuntimeError> {
        let Some(first_input_index) = self
            .input_condition_roots
            .iter()
            .position(|root| root.as_ref() == Some(condition_root))
        else {
            return Ok(None);
        };
        let Some(claim) = self
            .covenant_claims
            .iter()
            .find(|claim| claim.partition_input_index as usize == first_input_index)
        else {
            return Ok(None);
        };

        let input_commitments = self
            .inputs
            .iter()
            .zip(&self.input_condition_roots)
            .filter(|(_, root)| root.as_ref() == Some(condition_root))
            .map(|(input, _)| input.commitment)
            .collect::<Vec<_>>();
        // Only outputs re-locked under exactly `Script(condition_root)` stay in the partition; a `KeyAndScript` output
        // committing the same root carries a key-path escape, so its value is not conserved within the covenant (see
        // `StealthOutputView::is_locked_under`).
        let output_commitments = self
            .outputs
            .iter()
            .filter(|output| output.is_locked_under(condition_root))
            .map(|output| output.commitment)
            .collect::<Vec<_>>();

        charge(covenant_balance_proof_native_points(
            input_commitments.len(),
            output_commitments.len(),
        ))?;
        let valid = validate_covenant_balance_proof(
            condition_root,
            claim.revealed_amount,
            &input_commitments,
            &output_commitments,
            &claim.signature,
        );
        Ok(valid.then_some(claim.revealed_amount))
    }
}

/// The context of one `TemplateFunction` spend-script predicate: the shared statement view plus the input it gates.
#[derive(Debug, Clone)]
pub(crate) struct SpendScriptExecution {
    pub statement: Rc<StatementSpendView>,
    pub current_input_index: u32,
    pub current_input_commitment: PedersenCommitmentBytes,
    /// The committed `condition_root` of the UTXO whose leaf is currently executing. Keys the covenant partition.
    pub current_input_condition_root: Hash32,
    /// The raw spender-supplied witness `data` blob for the invoking input, exposed to a `TemplateFunction` predicate
    /// via `SpendContext::data`.
    pub witness_data: Vec<u8>,
}
