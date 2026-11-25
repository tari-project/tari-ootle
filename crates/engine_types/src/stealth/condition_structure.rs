//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::types::{
    access_rules::MAX_ACCESS_RULE_NODES,
    stealth::{AtomicCondition, SpendCondition},
};

use crate::limits;

/// A condition leaf whose shape the engine will reject at spend time.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConditionStructureError {
    #[error("Empty conjunction in spend condition")]
    EmptyConjunction,
    #[error("Conjunction has {num_conditions} conditions, exceeding the limit of {max_conditions}")]
    TooManyConditions {
        num_conditions: usize,
        max_conditions: usize,
    },
    #[error("Spend condition holds {num_nodes} nodes, exceeding the limit of {max_nodes}")]
    TooManyNodes { num_nodes: usize, max_nodes: usize },
    #[error("{num_consumers} data-consuming builtins; at most one may consume the witness data")]
    MultipleDataConsumers { num_consumers: usize },
    #[error(
        "pairs a data-consuming builtin with a TemplateFunction; a data-consuming builtin must be the sole consumer \
         of the witness data"
    )]
    DataConsumerWithTemplateFunction,
}

/// Validates a condition leaf's structure. This is the spend-time admissibility rule for a leaf, applied by the engine
/// before a revealed leaf is evaluated and by the wallet before funds are committed to a tree containing it — a leaf
/// that fails here can never be satisfied, so an output whose only spend paths are such leaves is unspendable.
///
/// A leaf is a conjunction of atoms (logical AND). It must be non-empty and must not exceed
/// `STEALTH_LIMITS.max_conditions_per_conjunction`, which caps the worst-case work of evaluating one leaf. It must hold
/// at most [`MAX_ACCESS_RULE_NODES`] nodes ([`SpendCondition::num_nodes`]), the bound `SpendCondition` decoding
/// applies, so a leaf that passes here can be revealed in a spend.
///
/// A data-consuming builtin (e.g. a hashlock) reads the entire witness `data` blob as its own raw input and cannot
/// know the blob's shape relative to siblings, so it must be the sole consumer of `data` in its leaf: a leaf may hold
/// at most one data-consuming builtin, and one may not share a leaf with a `TemplateFunction` (which may also read
/// `data`). Context-only conditions (timelocks, covenants, access rules) consume nothing and compose freely.
pub fn validate_condition_structure(leaf: &SpendCondition) -> Result<(), ConditionStructureError> {
    let conditions = leaf.conditions();
    if conditions.is_empty() {
        return Err(ConditionStructureError::EmptyConjunction);
    }

    let max_conditions = limits::STEALTH_LIMITS.max_conditions_per_conjunction;
    if conditions.len() > max_conditions {
        return Err(ConditionStructureError::TooManyConditions {
            num_conditions: conditions.len(),
            max_conditions,
        });
    }

    let num_nodes = leaf.num_nodes();
    if num_nodes > MAX_ACCESS_RULE_NODES {
        return Err(ConditionStructureError::TooManyNodes {
            num_nodes,
            max_nodes: MAX_ACCESS_RULE_NODES,
        });
    }

    let num_consumers = conditions.iter().filter(|c| c.is_data_owning_builtin()).count();
    if num_consumers > 1 {
        return Err(ConditionStructureError::MultipleDataConsumers { num_consumers });
    }
    if num_consumers == 1 && conditions.iter().any(AtomicCondition::is_template_function) {
        return Err(ConditionStructureError::DataConsumerWithTemplateFunction);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use tari_template_lib::types::{
        access_rules::AccessRule,
        stealth::{BuiltinPredicate, HashAlg, TemplateFunction},
    };

    use super::*;

    fn hashlock() -> AtomicCondition {
        AtomicCondition::Builtin(BuiltinPredicate::HashLock {
            hash: Default::default(),
            alg: HashAlg::Sha256,
        })
    }

    fn context_only() -> AtomicCondition {
        AtomicCondition::Builtin(BuiltinPredicate::AfterEpoch(1))
    }

    fn template_function() -> AtomicCondition {
        AtomicCondition::TemplateFunction(TemplateFunction {
            template: Default::default(),
            function: "predicate".try_into().unwrap(),
            args: Default::default(),
        })
    }

    #[test]
    fn accepts_a_conjunction_of_context_only_atoms() {
        let leaf = SpendCondition::all([context_only(), AtomicCondition::AccessRule(AccessRule::AllowAll)]);
        validate_condition_structure(&leaf).unwrap();
    }

    #[test]
    fn accepts_a_single_data_consumer_alongside_context_only_atoms() {
        let leaf = SpendCondition::all([hashlock(), context_only()]);
        validate_condition_structure(&leaf).unwrap();
    }

    #[test]
    fn rejects_an_empty_conjunction() {
        assert_eq!(
            validate_condition_structure(&SpendCondition::all([])),
            Err(ConditionStructureError::EmptyConjunction)
        );
    }

    #[test]
    fn rejects_a_conjunction_over_the_limit() {
        let max_conditions = limits::STEALTH_LIMITS.max_conditions_per_conjunction;
        let leaf = SpendCondition::all(vec![context_only(); max_conditions + 1]);
        assert_eq!(
            validate_condition_structure(&leaf),
            Err(ConditionStructureError::TooManyConditions {
                num_conditions: max_conditions + 1,
                max_conditions,
            })
        );
        // The limit itself is admissible.
        validate_condition_structure(&SpendCondition::all(vec![context_only(); max_conditions])).unwrap();
    }

    fn template_function_with_args(num_args: usize) -> AtomicCondition {
        AtomicCondition::TemplateFunction(TemplateFunction {
            template: Default::default(),
            function: "predicate".try_into().unwrap(),
            args: vec![Default::default(); num_args],
        })
    }

    fn decodes(leaf: &SpendCondition) -> bool {
        tari_bor::decode::<SpendCondition>(&tari_bor::encode(leaf).unwrap()).is_ok()
    }

    /// A leaf is admissible exactly when it decodes: a leaf that fails decoding can never be revealed in a spend.
    #[test]
    fn rejects_a_leaf_over_the_decode_node_limit() {
        let at_limit = SpendCondition::single(template_function_with_args(MAX_ACCESS_RULE_NODES - 1));
        assert_eq!(at_limit.num_nodes(), MAX_ACCESS_RULE_NODES);
        assert!(decodes(&at_limit));
        validate_condition_structure(&at_limit).unwrap();

        let over_limit = SpendCondition::single(template_function_with_args(MAX_ACCESS_RULE_NODES));
        assert!(!decodes(&over_limit));
        assert_eq!(
            validate_condition_structure(&over_limit),
            Err(ConditionStructureError::TooManyNodes {
                num_nodes: MAX_ACCESS_RULE_NODES + 1,
                max_nodes: MAX_ACCESS_RULE_NODES,
            })
        );

        // Atoms each within the limit whose sum exceeds it.
        let half = MAX_ACCESS_RULE_NODES / 2;
        let conjunction = SpendCondition::all([template_function_with_args(half), template_function_with_args(half)]);
        assert!(conjunction.num_nodes() > MAX_ACCESS_RULE_NODES);
        assert!(!decodes(&conjunction));
        assert!(matches!(
            validate_condition_structure(&conjunction),
            Err(ConditionStructureError::TooManyNodes { .. })
        ));
    }

    #[test]
    fn rejects_two_data_consumers_in_one_leaf() {
        assert_eq!(
            validate_condition_structure(&SpendCondition::all([hashlock(), hashlock()])),
            Err(ConditionStructureError::MultipleDataConsumers { num_consumers: 2 })
        );
    }

    #[test]
    fn rejects_a_data_consumer_sharing_a_leaf_with_a_template_function() {
        assert_eq!(
            validate_condition_structure(&SpendCondition::all([hashlock(), template_function()])),
            Err(ConditionStructureError::DataConsumerWithTemplateFunction)
        );
    }
}
