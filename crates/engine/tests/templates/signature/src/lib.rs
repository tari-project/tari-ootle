//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use tari_template_lib::prelude::*;

custom_signature_domain!(MyCustomDomain, b"tari.test.signature domain for tests");

const SOME_MESSAGE: &[u8] = b"Some message that binds to something important";

#[template]
mod template {
    use super::*;

    pub struct SignatureTest {
        allow_list: HashSet<PublicKey>,
        manager: ResourceManager,
        supply_vault: Vault,
    }

    impl SignatureTest {
        pub fn new(allow_list: HashSet<PublicKey>) -> Component<Self> {
            let bucket = ResourceBuilder::stealth()
                .with_token_symbol("SIGCOIN")
                .with_divisibility(9)
                .mintable(rule!(allow_all))
                .initial_supply(amount!(1000000000000000000000));

            let resource_address = bucket.resource_address();
            let supply_vault = Vault::from_bucket(bucket);

            Component::new(Self {
                allow_list,
                manager: resource_address.into(),
                supply_vault,
            })
            .with_access_rules(AccessRules::allow_all())
            .create()
        }

        pub fn check_sig(public_key: PublicKey, spend_signature: Signature<MyCustomDomain>) -> bool {
            // Mainly checking the conditional API works i.e. does not panic and returns the expected result.
            spend_signature.verify(&public_key, SOME_MESSAGE)
        }

        pub fn claim_funds(
            &mut self,
            public_key: PublicKey,
            spend_signature: Signature<MyCustomDomain>,
            transfer: StealthTransferStatement,
        ) {
            if !transfer.revealed_input_amount().is_positive() {
                panic!("Input amount must be positive");
            }
            if transfer.revealed_input_amount() > 1000_000_000_000u64 {
                panic!("Cannot claim more than 1000 SIGCOIN at a time");
            }
            // 1. Remove the public key from the allow list to prevent double claims
            assert!(
                self.allow_list.remove(&public_key),
                "Public key {public_key} is not in the allow list"
            );

            // 2. Verify that the signature is valid for the public key and the message
            // Note: that to prevent replay attacks, some single-use data would need to be included in the message. In
            // this case, the public key is removed from the allow list, so it can only be used once.
            // A nonce field on the component could also be used.
            // This template is about testing signature verification, so the double claim mechanism isn't tested.
            if !spend_signature.verify(&public_key.into(), SOME_MESSAGE) {
                panic!("Your signature is invalid, so no funds for you");
            }

            let input_bucket = self.supply_vault.withdraw(transfer.inputs_statement.revealed_amount);

            let bucket = self
                .manager
                .stealth_transfer_with_opt_input_bucket(transfer, Some(input_bucket));
            if let Some(bucket) = bucket {
                // Any revealed funds are transferred back into the component's vault.
                self.supply_vault.deposit(bucket);
            }
        }
    }
}
