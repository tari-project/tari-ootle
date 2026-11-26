//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::{EngineOp, call_engine};
use tari_template_lib::{
    args::{ComponentAction, ComponentInvokeArg, ComponentRef, CreateComponentArg, InvokeResult},
    prelude::*,
    types::SubstateOwnerRule,
};

#[template]
mod template {
    use super::*;

    /// A component whose `guarded` method denies every caller but the owner until its access rules change.
    pub struct RulesUpdaterTest {
        value: u32,
    }

    impl RulesUpdaterTest {
        pub fn new(owner_rule: OwnerRule, rules_updater: RulesUpdater) -> Component<Self> {
            Component::new(Self { value: 0 })
                .with_owner_rule(owner_rule)
                .with_access_rules(initial_access_rules())
                .with_rules_updater(rules_updater)
                .create()
        }

        /// Creates the component the way a template built before the rules updater existed does: the creation
        /// argument carries no rules updater.
        pub fn new_without_rules_updater(owner_rule: OwnerRule) -> ComponentAddress {
            let result = call_engine::<_, InvokeResult>(EngineOp::ComponentInvoke, &ComponentInvokeArg {
                component_ref: ComponentRef::Component,
                action: ComponentAction::Create,
                args: args![CreateComponentArg {
                    encoded_state: tari_template_lib::to_value(&Self { value: 0 }).unwrap(),
                    owner_rule,
                    access_rules: initial_access_rules(),
                    address_allocation: None,
                    upgrade_rule: UpdateRule::Locked,
                    rules_updater: None,
                }],
            });
            result.decode().unwrap()
        }

        pub fn guarded(&mut self) {
            self.value += 1;
        }

        pub fn set_access_rules_from_template(&mut self, access_rules: ComponentAccessRules) {
            ComponentManager::current().set_access_rules(access_rules);
        }

        pub fn set_owner_rule_from_template(&mut self, owner_rule: SubstateOwnerRule) {
            ComponentManager::current().set_owner_rule(owner_rule);
        }
    }

    fn initial_access_rules() -> ComponentAccessRules {
        ComponentAccessRules::new()
            .default(AccessRule::AllowAll)
            .add_method_rule("guarded", AccessRule::DenyAll)
    }
}
