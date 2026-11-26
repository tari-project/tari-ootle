//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

/// Callee template: `bar` is restricted to the component whose address is passed as `gate`.
#[template]
mod callee {
    use super::*;

    pub struct Callee;

    impl Callee {
        pub fn new(gate: ComponentAddress) -> Component<Self> {
            let access_rules = ComponentAccessRules::new()
                .method("bar", rule!(caller_component(gate)))
                .method("ping", rule!(allow_all))
                .default(rule!(deny_all));

            Component::new(Callee)
                .with_access_rules(access_rules)
                .with_owner_rule(OwnerRule::None)
                .create()
        }

        pub fn new_self_gated() -> Component<Self> {
            // Allocate the address first so it can gate the component's own method.
            let allocation = CallerContext::allocate_component_address(None);
            let own_address = allocation.get_address();
            let access_rules = ComponentAccessRules::new()
                .method("bar", rule!(caller_component(own_address)))
                .method("ping", rule!(allow_all))
                .default(rule!(deny_all));

            Component::new(Callee)
                .with_address_allocation(allocation)
                .with_access_rules(access_rules)
                .with_owner_rule(OwnerRule::None)
                .create()
        }

        pub fn new_owner_gated(gate: ComponentAddress) -> Component<Self> {
            // Ownership is gated on the caller component, while `bar` is left under the default
            // `deny_all`. Only the owner (the gated caller) can reach `bar` via the ownership
            // short-circuit; everyone else is denied by the method rule.
            let access_rules = ComponentAccessRules::new()
                .method("ping", rule!(allow_all))
                .default(rule!(deny_all));

            Component::new(Callee)
                .with_access_rules(access_rules)
                .with_owner_rule(OwnerRule::ByAccessRule(rule!(caller_component(gate))))
                .with_rules_updater(RulesUpdater::Owner)
                .create()
        }

        pub fn bar(&self) {}

        /// Opens `bar` to everyone. Updating access rules requires ownership of this component.
        pub fn open_bar(&mut self) {
            ComponentManager::current().set_access_rules(
                ComponentAccessRules::new()
                    .method("bar", rule!(allow_all))
                    .method("ping", rule!(allow_all))
                    .default(rule!(deny_all)),
            );
        }

        pub fn ping(&self) -> u64 {
            42
        }
    }
}
