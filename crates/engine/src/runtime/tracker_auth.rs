//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::cell::Cell;

use tari_engine_types::{limits::NativeExecutionPoints, ownership::Ownership};
use tari_template_lib::types::{
    NonFungibleAddress,
    SubstateOwnerRule,
    access_rules::{
        AccessRule,
        RequireRule,
        ResourceAccessRules,
        ResourceAuthAction,
        RestrictedAccessRule,
        RuleRequirement,
    },
};

use crate::{
    runtime::{ActionIdent, AuthorizationScope, RuntimeError, working_state::WorkingState},
    state_store::StateReader,
};

/// Meters the work of evaluating access rules: each rule node visited and each proof examined.
///
/// Evaluation runs while the working state is borrowed, where the tracker cannot charge, and its cost follows the
/// rule's size and the proofs in scope. The tracker opens the meter with the most the transaction can still pay for
/// before it borrows the working state, and charges what was used once the borrow ends. Evaluation stops as soon as it
/// would use more than that budget, so every rule is evaluated only as far as the transaction can pay for.
#[derive(Debug, Clone, Default)]
pub struct AuthorizationMeter {
    /// `None` while closed. Evaluating an access rule requires an open meter and fails with the meter closed.
    budget: Cell<Option<u64>>,
    used: Cell<u64>,
}

impl AuthorizationMeter {
    pub(super) fn open(&self, budget: u64) {
        self.budget.set(Some(budget));
        self.used.set(0);
    }

    /// Closes the meter, returning the points used since it was opened: more than the budget it was opened with when
    /// evaluation ran out of it.
    pub(super) fn close(&self) -> u64 {
        self.budget.set(None);
        self.used.take()
    }

    fn charge(&self, points: u64) -> Result<(), RuntimeError> {
        let budget = self.budget.get().ok_or_else(|| RuntimeError::InvariantError {
            function: "AuthorizationMeter::charge",
            details: "access rule evaluated without an open authorization meter".to_string(),
        })?;
        let used = self.used.get().saturating_add(points);
        self.used.set(used);
        if used > budget {
            // The tracker charges `used` once the working state is released, and that charge fails with the
            // transaction's own out-of-compute error, which is the one reported.
            return Err(RuntimeError::InvariantError {
                function: "AuthorizationMeter::charge",
                details: format!("access rule evaluation used {used} points of a {budget} point budget"),
            });
        }
        Ok(())
    }
}

pub struct Authorization<'a, TStore> {
    state: &'a WorkingState<TStore>,
    meter: &'a AuthorizationMeter,
}

impl<'a, TStore: StateReader> Authorization<'a, TStore> {
    pub(super) fn new(state: &'a WorkingState<TStore>, meter: &'a AuthorizationMeter) -> Self {
        Self { state, meter }
    }

    fn evaluator(&self) -> Result<RuleEvaluator<'a, TStore>, RuntimeError> {
        Ok(RuleEvaluator {
            state: self.state,
            scope: self.state.current_call_scope()?.auth_scope(),
            meter: self.meter,
        })
    }

    pub fn check_current_component_access_rules(&self, method: &str) -> Result<(), RuntimeError> {
        let locked = self
            .state
            .current_call_scope()?
            .get_current_component_lock()
            .ok_or_else(|| RuntimeError::InvariantError {
                function: "check_component_access_rules",
                details: "No current component lock in call scope".to_string(),
            })?;
        let component = self.state.get_component(locked)?;
        let evaluator = self.evaluator()?;

        if evaluator.check_ownership(component.as_ownership())? {
            // Owner can call any component method
            return Ok(());
        }

        let component_address =
            locked
                .substate_id()
                .as_component_address()
                .ok_or_else(|| RuntimeError::InvariantError {
                    function: "check_component_access_rules",
                    details: format!("Expected a component address, got {}", locked.substate_id()),
                })?;

        let access_rule = component.access_rules().get_method_access_rule(method);
        if !evaluator.check_access_rule(access_rule)? {
            return Err(RuntimeError::AccessDenied {
                action_ident: ActionIdent::ComponentCallMethod {
                    component_address,
                    method: method.to_string(),
                },
            });
        }
        Ok(())
    }

    pub fn check_resource_access_rules(
        &self,
        action: ResourceAuthAction,
        resource_ownership: Ownership<'_>,
        resource_access_rules: &ResourceAccessRules,
    ) -> Result<(), RuntimeError> {
        let evaluator = self.evaluator()?;

        // Check ownership.
        // A resource is only recallable by explicit access rules
        if !action.is_recall() && evaluator.check_ownership(resource_ownership)? {
            // Owner can invoke any resource method
            return Ok(());
        }

        let rule = resource_access_rules.get_access_rule(&action);
        if !evaluator.check_access_rule(rule)? {
            return Err(RuntimeError::AccessDenied {
                action_ident: action.into(),
            });
        }

        Ok(())
    }

    pub fn check_access_rule(&self, rule: &AccessRule) -> Result<bool, RuntimeError> {
        self.evaluator()?.check_access_rule(rule)
    }

    /// Returns `true` if the current frame satisfies `ownership`. Every authorization check is evaluated against the
    /// current frame's scope: a component's own frame is already on top of the stack when its ownership rule is
    /// checked, and that scope carries its caller's identity as virtual badges.
    pub fn check_ownership(&self, ownership: Ownership<'_>) -> Result<bool, RuntimeError> {
        self.evaluator()?.check_ownership(ownership)
    }

    /// Requires that the current frame satisfies `ownership`.
    pub fn require_ownership<A: Into<ActionIdent>>(
        &self,
        action: A,
        ownership: Ownership<'_>,
    ) -> Result<(), RuntimeError> {
        if !self.check_ownership(ownership)? {
            return Err(RuntimeError::AccessDeniedOwnerRequired { action: action.into() });
        }
        Ok(())
    }
}

/// Evaluates rules against one authorization scope, charging the meter for every node it visits and every proof it
/// examines before doing that work.
struct RuleEvaluator<'a, TStore> {
    state: &'a WorkingState<TStore>,
    scope: &'a AuthorizationScope,
    meter: &'a AuthorizationMeter,
}

impl<TStore: StateReader> RuleEvaluator<'_, TStore> {
    fn visit_node(&self) -> Result<(), RuntimeError> {
        self.meter.charge(NativeExecutionPoints::PER_ACCESS_RULE_NODE)
    }

    fn examine_proof(&self) -> Result<(), RuntimeError> {
        self.meter.charge(NativeExecutionPoints::PER_ACCESS_RULE_PROOF)
    }

    fn check_ownership(&self, ownership: Ownership<'_>) -> Result<bool, RuntimeError> {
        self.visit_node()?;
        match ownership.owner_rule.as_ref() {
            SubstateOwnerRule::None => Ok(false),
            SubstateOwnerRule::ByAccessRule(rule) => self.check_access_rule(rule),
            SubstateOwnerRule::ByPublicKey(key) => {
                let owner_proof = NonFungibleAddress::from_public_key(*key);
                Ok(self.scope.contains_badge(&owner_proof))
            },
        }
    }

    fn check_access_rule(&self, rule: &AccessRule) -> Result<bool, RuntimeError> {
        self.visit_node()?;
        match rule {
            AccessRule::AllowAll => Ok(true),
            AccessRule::DenyAll => Ok(false),
            AccessRule::Restricted(rule) => self.check_restricted_access_rule(rule),
        }
    }

    fn check_restricted_access_rule(&self, rule: &RestrictedAccessRule) -> Result<bool, RuntimeError> {
        self.visit_node()?;
        match rule {
            RestrictedAccessRule::Require(rule) => self.check_require_rule(rule),
            RestrictedAccessRule::AnyOf(rules) => {
                for rule in rules {
                    if self.check_restricted_access_rule(rule)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            },
            RestrictedAccessRule::AllOf(rules) => {
                // Empty AllOf is denied rather than vacuously true, to prevent accidental AllowAll
                if rules.is_empty() {
                    return Ok(false);
                }
                for rule in rules {
                    if !self.check_restricted_access_rule(rule)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            },
        }
    }

    fn check_require_rule(&self, rule: &RequireRule) -> Result<bool, RuntimeError> {
        self.visit_node()?;
        match rule {
            RequireRule::Require(requirement) => self.check_requirement(requirement),
            RequireRule::AnyOf(requirements) => {
                for requirement in requirements {
                    if self.check_requirement(requirement)? {
                        return Ok(true);
                    }
                }

                Ok(false)
            },
            RequireRule::AllOf(requirements) => {
                // Empty AllOf is denied rather than vacuously true, to prevent accidental AllowAll
                if requirements.is_empty() {
                    return Ok(false);
                }
                for requirement in requirements {
                    if !self.check_requirement(requirement)? {
                        return Ok(false);
                    }
                }

                Ok(true)
            },
            RequireRule::MOfN(n, requirements) => {
                // The engine rejects an invalid MOfN rule (threshold of zero, above `requirements.len()`, or duplicate
                // requirements) wherever it stores a rule. A stealth output commits only to a hash of its spend
                // conditions, so its rules are first seen here and an invalid MOfN rule must fail closed.
                if rule.find_invalid_rule().is_some() {
                    return Ok(false);
                }
                let mut satisfied = 0u16;
                for requirement in requirements {
                    if self.check_requirement(requirement)? {
                        satisfied += 1;
                        if satisfied == *n {
                            return Ok(true);
                        }
                    }
                }

                Ok(false)
            },
        }
    }

    fn check_requirement(&self, requirement: &RuleRequirement) -> Result<bool, RuntimeError> {
        self.visit_node()?;
        let RuleEvaluator { state, scope, .. } = *self;
        match requirement {
            RuleRequirement::Resource(resx) => {
                if scope.contains_badge_of_resource(resx) {
                    return Ok(true);
                }

                for proof_id in scope.authorizing_proofs() {
                    self.examine_proof()?;
                    let proof = state.get_proof(*proof_id)?;

                    // A resource rule requires possession, so the proof must hold some of the resource.
                    if resx == proof.resource_address() && !proof.is_empty() {
                        return Ok(true);
                    }
                }
                Ok(false)
            },
            RuleRequirement::NonFungibleAddress(addr) => {
                if scope.contains_badge(addr) {
                    return Ok(true);
                }

                for proof_id in scope.authorizing_proofs() {
                    self.examine_proof()?;
                    let proof = state.get_proof(*proof_id)?;

                    if addr.resource_address() != proof.resource_address() {
                        continue;
                    }
                    self.meter
                        .charge(NativeExecutionPoints::PER_ACCESS_RULE_NON_FUNGIBLE_LOOKUP)?;
                    if proof.non_fungible_token_ids().contains(addr.id()) {
                        return Ok(true);
                    }
                }

                Ok(false)
            },
            // `ScopedToComponent` / `ScopedToTemplate` mean "execution is within this component/template":
            // they always describe the current (top) frame, which is the actor on resource/ownership checks
            // and the callee on method checks. Outside any frame (a top-level instruction) no component or template
            // is executing, so both are false.
            RuleRequirement::ScopedToComponent(_) | RuleRequirement::ScopedToTemplate(_)
                if state.call_frame_depth() == 0 =>
            {
                Ok(false)
            },
            RuleRequirement::ScopedToComponent(address) => Ok(state.current_component()? == Some(*address)),
            RuleRequirement::ScopedToTemplate(address) => Ok(state.current_template()? == address),
            // Sugar for the caller badges the engine stamps into a frame's scope at push. Both badge resources are
            // empty by invariant, so the scope's badges are the only thing that can satisfy these.
            RuleRequirement::CallerComponent(address) => {
                Ok(scope.contains_badge(&NonFungibleAddress::caller_component_badge(*address)))
            },
            RuleRequirement::DirectCallerTemplate(address) => {
                Ok(scope.contains_badge(&NonFungibleAddress::direct_caller_template_badge(*address)))
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use ootle_network::Network;
    use tari_engine_types::{
        fees::{ExhaustBurnRate, FeeScale},
        indexed_value::IndexedWellKnownTypes,
        virtual_substate::VirtualSubstates,
    };
    use tari_template_lib::types::{
        ComponentAddress,
        EntityId,
        Hash32,
        ObjectKey,
        ResourceAddress,
        TemplateAddress,
        access_rules::UpdateRule,
    };

    use super::*;
    use crate::{
        runtime::scope::{CallScope, PushCallFrame},
        state_store::memory::MemoryStateStore,
    };

    fn test_working_state() -> WorkingState<MemoryStateStore> {
        WorkingState::new(
            MemoryStateStore::new(),
            VirtualSubstates::empty(),
            CallScope::new(),
            HashSet::new(),
            Hash32::zero(),
            Hash32::zero(),
            ExhaustBurnRate::new(0),
            FeeScale::default(),
            Network::LocalNet,
            false,
        )
    }

    #[test]
    fn scoped_requirements_evaluate_to_false_outside_call_frame() {
        let state = test_working_state();
        let meter = AuthorizationMeter::default();
        meter.open(u64::MAX);
        let auth = Authorization::new(&state, &meter);

        let component_address = ComponentAddress::from_array([1u8; ObjectKey::LENGTH]);
        let template_address = TemplateAddress::from_array([2u8; 32]);
        let badge_resource = ResourceAddress::new(ObjectKey::from_array([3u8; ObjectKey::LENGTH]));

        let comp_rule = AccessRule::Restricted(RestrictedAccessRule::Require(RequireRule::Require(
            RuleRequirement::ScopedToComponent(component_address),
        )));
        assert!(!auth.check_access_rule(&comp_rule).unwrap());

        let tmpl_rule = AccessRule::Restricted(RestrictedAccessRule::Require(RequireRule::Require(
            RuleRequirement::ScopedToTemplate(template_address),
        )));
        assert!(!auth.check_access_rule(&tmpl_rule).unwrap());

        let any_of_rule = AccessRule::Restricted(RestrictedAccessRule::Require(RequireRule::AnyOf(Box::new([
            RuleRequirement::ScopedToComponent(component_address),
            RuleRequirement::ScopedToTemplate(template_address),
            RuleRequirement::Resource(badge_resource),
        ]))));
        assert!(!auth.check_access_rule(&any_of_rule).unwrap());

        let owner_rule = SubstateOwnerRule::ByAccessRule(comp_rule);
        let access_rules = ResourceAccessRules::new().withdrawable(AccessRule::AllowAll, UpdateRule::Locked);
        auth.check_resource_access_rules(
            ResourceAuthAction::Withdraw,
            Ownership {
                owner_rule: std::borrow::Cow::Borrowed(&owner_rule),
            },
            &access_rules,
        )
        .unwrap();
    }

    #[test]
    fn scoped_to_template_matches_active_call_frame() {
        let mut state = test_working_state();
        let template_address = TemplateAddress::from_array([2u8; 32]);
        let other_template = TemplateAddress::from_array([3u8; 32]);

        let frame = PushCallFrame::Static {
            template_address,
            module_name: "Test".to_string(),
            arg_scope: IndexedWellKnownTypes::new(),
            entity_id: EntityId::default(),
        }
        .into_new_call_frame();
        state.push_frame(frame, 10).unwrap();

        let meter = AuthorizationMeter::default();
        meter.open(u64::MAX);
        let auth = Authorization::new(&state, &meter);

        let matching = AccessRule::Restricted(RestrictedAccessRule::Require(RequireRule::Require(
            RuleRequirement::ScopedToTemplate(template_address),
        )));
        let non_matching = AccessRule::Restricted(RestrictedAccessRule::Require(RequireRule::Require(
            RuleRequirement::ScopedToTemplate(other_template),
        )));

        assert!(auth.check_access_rule(&matching).unwrap());
        assert!(!auth.check_access_rule(&non_matching).unwrap());
    }
}
