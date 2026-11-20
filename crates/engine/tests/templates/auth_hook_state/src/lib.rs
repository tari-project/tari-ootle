//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

/// A hooked resource and components that hold it. The resource's auth hook runs whenever a holder acts on its
/// vault, and receives the holder's state in the `AuthHookCaller`.
#[template]
mod auth_hook_state {
    use tari_template_lib::types::FunctionName;

    use super::*;

    pub struct AuthHookState {
        tokens: Vault,
        /// A resource this component created, which only this component's state names.
        own_resource: ResourceAddress,
        other: Option<ComponentAddress>,
        recorded: Option<ResourceAddress>,
    }

    impl AuthHookState {
        /// Creates the hooked resource, with `hook` on this component as its auth hook. This component holds the
        /// whole supply.
        pub fn new(hook: FunctionName) -> Component<Self> {
            let alloc = CallerContext::allocate_component_address(None);
            let tokens = ResourceBuilder::public_fungible()
                .with_authorization_hook(alloc.get_address(), hook)
                .initial_supply(1000u32);

            Component::new(Self {
                tokens: Vault::from_bucket(tokens),
                own_resource: ResourceBuilder::public_fungible().build(),
                other: None,
                recorded: None,
            })
            .with_address_allocation(alloc)
            .with_owner_rule(OwnerRule::None)
            .with_access_rules(ComponentAccessRules::allow_all())
            .create()
        }

        /// Creates a component holding `tokens` whose state names `other`.
        pub fn holder(tokens: Bucket, other: Option<ComponentAddress>) -> Component<Self> {
            Component::new(Self {
                tokens: Vault::from_bucket(tokens),
                own_resource: ResourceBuilder::public_fungible().build(),
                other,
                recorded: None,
            })
            .with_owner_rule(OwnerRule::None)
            .with_access_rules(ComponentAccessRules::allow_all())
            .create()
        }

        pub fn take_tokens(&mut self, amount: Amount) -> Bucket {
            self.tokens.withdraw(amount)
        }

        /// Withdraws one token and deposits it back, so the resource's hook runs for both actions.
        pub fn cycle(&mut self) {
            let bucket = self.tokens.withdraw(Amount::new(1));
            self.tokens.deposit(bucket);
        }

        pub fn accept_hook(&self, _action: ResourceAuthAction, _caller: AuthHookCaller) {}

        /// Records the acting component's own resource, which this hook learns only from the acting component's
        /// state, in this component's state.
        pub fn hook_records_caller_resource(&mut self, _action: ResourceAuthAction, caller: AuthHookCaller) {
            if let Some(state) = caller.component_state() {
                let state: AuthHookState = tari_bor::from_value(state).expect("caller state is an AuthHookState");
                self.recorded = Some(state.own_resource);
            }
        }
    }
}
