//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

/// Exercises how a proof's authority moves between frames: a held proof authorizes nothing until a window is opened
/// over it, a window lends the proof one hop, and a proof passed as an argument is shown and read-only.
#[template]
mod template {
    use super::*;

    pub struct ProofWindow {
        /// Non-fungible badges; `tokens` may be withdrawn only by a frame carrying one.
        badges: Vault,
        tokens: Vault,
    }

    impl ProofWindow {
        pub fn new() -> Component<Self> {
            let badges = ResourceBuilder::non_fungible().initial_supply([NonFungibleId::from_string("badge")]);
            let tokens = ResourceBuilder::public_fungible()
                .with_owner_rule(OwnerRule::None)
                .withdrawable(rule!(resource(badges.resource_address())), LOCKED)
                .initial_supply(1000u32);
            Component::new(Self {
                badges: Vault::from_bucket(badges),
                tokens: Vault::from_bucket(tokens),
            })
            .with_access_rules(AccessRules::allow_all())
            .with_owner_rule(OwnerRule::None)
            .create()
        }

        /// A component whose tokens may be withdrawn only by a frame carrying a badge of `badge_resource`, which
        /// another component holds.
        pub fn new_gated_by(badge_resource: ResourceAddress) -> Component<Self> {
            let tokens = ResourceBuilder::public_fungible()
                .with_owner_rule(OwnerRule::None)
                .withdrawable(rule!(resource(badge_resource)), LOCKED)
                .initial_supply(1000u32);
            Component::new(Self {
                badges: Vault::new_empty(badge_resource),
                tokens: Vault::from_bucket(tokens),
            })
            .with_access_rules(AccessRules::allow_all())
            .with_owner_rule(OwnerRule::None)
            .create()
        }

        pub fn badge(&self) -> NonFungibleAddress {
            NonFungibleAddress::new(self.badges.resource_address(), NonFungibleId::from_string("badge"))
        }

        pub fn tokens_resource(&self) -> ResourceAddress {
            self.tokens.resource_address()
        }

        /// A proof of the badge, handed back held so the transaction can open a window over it.
        pub fn create_badge_proof(&self) -> Proof {
            self.badges.create_proof()
        }

        /// The action `tokens`' withdraw rule guards. Satisfied by this frame's own window or by a proof its caller
        /// lent it.
        pub fn take_tokens(&self, amount: Amount) -> Bucket {
            self.tokens.withdraw(amount)
        }

        pub fn deposit_tokens(&self, bucket: Bucket) {
            self.tokens.deposit(bucket);
        }

        /// Holds a proof of the badge while withdrawing, without opening a window over it.
        pub fn take_tokens_holding_proof(&self, amount: Amount) -> Bucket {
            let proof = self.badges.create_proof();
            let bucket = self.tokens.withdraw(amount);
            proof.drop();
            bucket
        }

        /// Withdraws inside a window over a proof of the badge.
        pub fn take_tokens_in_window(&self, amount: Amount) -> Bucket {
            self.badges.authorize_with(|| self.tokens.withdraw(amount))
        }

        /// Calls `method` on `other` inside a window over a proof of this component's badge: `other` is lent it.
        pub fn lend_badge_to(&self, other: ComponentAddress, method: String, args: Vec<Bytes>) -> tari_bor::Value {
            self.badges
                .authorize_with(|| ComponentManager::get(other).call(method, args))
        }

        /// Calls `method` on `other` while merely holding a proof of this component's badge.
        pub fn call_holding_badge(&self, other: ComponentAddress, method: String, args: Vec<Bytes>) -> tari_bor::Value {
            let proof = self.badges.create_proof();
            let result = ComponentManager::get(other).call(method, args);
            proof.drop();
            result
        }

        /// Calls `method` on `other` with whatever this frame was lent, lending nothing of its own.
        pub fn relay(&self, other: ComponentAddress, method: String, args: Vec<Bytes>) -> tari_bor::Value {
            ComponentManager::get(other).call(method, args)
        }

        /// Asserts that this call carries the badge in `badges`, the one `tokens`' withdraw rule names.
        pub fn assert_badge_in_scope(&self) {
            CallerContext::assert_badge_in_scope(&self.badge());
        }

        /// Asserts the badge while being shown `proof`. A shown proof does not carry its badge into this frame.
        pub fn assert_badge_in_scope_shown(&self, proof: Proof) {
            proof.assert_resource(self.badges.resource_address());
            CallerContext::assert_badge_in_scope(&self.badge());
        }

        /// Reads a shown proof.
        pub fn inspect(&self, proof: Proof) -> (ResourceAddress, Amount, Vec<NonFungibleId>) {
            (
                proof.resource_address(),
                proof.amount(),
                proof.get_non_fungibles().into_iter().collect(),
            )
        }

        /// Whether a second window can be opened over a proof whose window is already open.
        pub fn try_nested_window(&self) -> bool {
            let auth = self.badges.authorize();
            auth.proof().try_authorize().is_ok()
        }

        /// Whether a shown proof can be authorized here.
        pub fn try_authorize_shown(&self, proof: Proof) -> bool {
            proof.try_authorize().is_ok()
        }

        pub fn pass_on_shown(&self, other: ComponentAddress, proof: Proof) {
            let _: (ResourceAddress, Amount, Vec<NonFungibleId>) =
                ComponentManager::get(other).call("inspect", args![proof]);
        }

        pub fn return_shown(&self, proof: Proof) -> Proof {
            proof
        }

        pub fn drop_shown(&self, proof: Proof) {
            proof.drop();
        }
    }
}
