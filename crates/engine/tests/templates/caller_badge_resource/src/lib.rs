//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

/// A resource whose withdraw rule is gated on the caller of whichever component holds it, held in a
/// vault by a component whose methods are open to everyone. Only the resource rule decides who may move
/// the tokens, so the gate applies to every holder rather than to one hand-written method gate.
#[template]
mod gated_resource {
    use super::*;

    pub struct GatedResource {
        vault: Vault,
        stored_rule: Option<AccessRule>,
    }

    impl GatedResource {
        pub fn new(gate: ComponentAddress) -> Component<Self> {
            let tokens = ResourceBuilder::public_fungible()
                .with_owner_rule(OwnerRule::None)
                .withdrawable(rule!(caller_component(gate)), LOCKED)
                .initial_supply(1000u32);

            Component::new(GatedResource {
                vault: Vault::from_bucket(tokens),
                stored_rule: None,
            })
            .with_owner_rule(OwnerRule::None)
            .with_access_rules(ComponentAccessRules::allow_all())
            .create()
        }

        /// As [`Self::new`], but gated on the caller's *template* so that a static function of `gate`
        /// satisfies the rule as well as a component instance of it.
        pub fn new_template_gated(gate: TemplateAddress) -> Component<Self> {
            let tokens = ResourceBuilder::public_fungible()
                .with_owner_rule(OwnerRule::None)
                .withdrawable(rule!(direct_caller_template(gate)), LOCKED)
                .initial_supply(1000u32);

            Component::new(GatedResource {
                vault: Vault::from_bucket(tokens),
                stored_rule: None,
            })
            .with_owner_rule(OwnerRule::None)
            .with_access_rules(ComponentAccessRules::allow_all())
            .create()
        }

        /// A resource whose mint rule is gated on the caller of whichever component mints it.
        pub fn new_mint_gated(gate: ComponentAddress) -> Component<Self> {
            let tokens = ResourceBuilder::public_fungible()
                .with_owner_rule(OwnerRule::None)
                .mintable(rule!(caller_component(gate)), LOCKED)
                .initial_supply(1000u32);

            Component::new(GatedResource {
                vault: Vault::from_bucket(tokens),
                stored_rule: None,
            })
            .with_owner_rule(OwnerRule::None)
            .with_access_rules(ComponentAccessRules::allow_all())
            .create()
        }

        /// Accepts a caller-supplied `withdraw_rule`, stores it in component state, and gates withdrawals on it.
        pub fn new_with_withdraw_rule(withdraw_rule: AccessRule) -> Component<Self> {
            let tokens = ResourceBuilder::public_fungible()
                .with_owner_rule(OwnerRule::None)
                .withdrawable(withdraw_rule.clone(), LOCKED)
                .initial_supply(1000u32);

            Component::new(GatedResource {
                vault: Vault::from_bucket(tokens),
                stored_rule: Some(withdraw_rule),
            })
            .with_owner_rule(OwnerRule::None)
            .with_access_rules(ComponentAccessRules::allow_all())
            .create()
        }

        pub fn stored_rule(&self) -> Option<AccessRule> {
            self.stored_rule.clone()
        }

        pub fn resource_address(&self) -> ResourceAddress {
            self.vault.resource_address()
        }

        /// Withdraws and immediately returns the tokens, so the call succeeds or fails purely on the
        /// resource's withdraw rule.
        pub fn withdraw_once(&mut self) {
            let bucket = self.vault.withdraw(Amount::new(1));
            self.vault.deposit(bucket);
        }
    }
}
