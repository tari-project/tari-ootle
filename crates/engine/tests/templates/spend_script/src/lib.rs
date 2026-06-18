//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Example spend-script predicates (TIP-0006).
//!
//! A spend script is an ordinary `is_mut == false` template function whose last argument is a `SpendContext`. It
//! authorises a spend by returning normally and rejects it by panicking. These predicates exercise the various
//! spend-script capabilities: timelocks, recursive covenants, signature locks, and the read-only sandbox. The
//! deliberately ill-shaped functions at the end exist to exercise creation-time (T1) signature validation.

use tari_template_lib::{engine, prelude::*};

custom_signature_domain!(SpendSigDomain, b"tari.test.spend_script signature domain");

/// The fixed message a `require_signature` spend script verifies its bound signature against.
const SIG_MESSAGE: &[u8] = b"spend authorisation";

#[template]
mod spend_scripts {
    use super::*;

    pub struct SpendScripts {
        nonce: u64,
    }

    impl SpendScripts {
        pub fn new() -> Component<Self> {
            Component::new(Self { nonce: 0 })
                .with_access_rules(AccessRules::allow_all())
                .create()
        }

        // ------------------------------ Spend-script predicates ------------------------------ //

        /// Unconditionally authorises the spend.
        pub fn always_ok(_ctx: SpendContext) {}

        /// Unconditionally rejects the spend.
        pub fn always_reject(_ctx: SpendContext) {
            panic!("spend always rejected");
        }

        /// Absolute timelock: rejects until `unlock_epoch`.
        pub fn timelock(unlock_epoch: u64, ctx: SpendContext) {
            ctx.require_timelock(unlock_epoch);
        }

        /// Recursive covenant: every output must carry this same spend condition.
        pub fn preserve_covenant(ctx: SpendContext) {
            ctx.require_output_preserves_condition();
        }

        /// Signature lock: authorises only if the bound `signature` is valid for `public_key` over a fixed message.
        /// Exercises `signature_invoke` from inside a spend script.
        pub fn require_signature(public_key: PublicKey, signature: Signature<SpendSigDomain>, _ctx: SpendContext) {
            assert!(signature.verify(&public_key, SIG_MESSAGE), "invalid spend signature");
        }

        /// Attempts to mutate ledger state by creating a component. The read-only spend-script sandbox refuses the
        /// underlying `new_substate`/write-lock, so this always aborts the spend.
        pub fn try_write(_ctx: SpendContext) {
            engine().create_component(0u32, OwnerRule::default(), AccessRules::allow_all(), None);
        }

        /// Attempts to emit an event, which is on the spend-script deny-list.
        pub fn try_emit_event(_ctx: SpendContext) {
            emit_event("spend_script_test", Metadata::new());
        }

        // --------------- Deliberately ill-shaped functions for creation-time (T1) tests --------------- //

        /// Mutable predicate — rejected at creation time (`is_mut == true`).
        pub fn bad_mutable(&mut self, _ctx: SpendContext) {
            self.nonce += 1;
        }

        /// Non-unit return — rejected at creation time.
        pub fn bad_returns_value(_ctx: SpendContext) -> u32 {
            0
        }

        /// Missing trailing `SpendContext` argument — rejected at creation time.
        pub fn bad_no_context(_x: u64) {}
    }
}
