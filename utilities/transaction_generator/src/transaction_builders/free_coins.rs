//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use rand::rngs::OsRng;
use tari_crypto::{keys::PublicKey, ristretto::RistrettoPublicKey};
use tari_ootle_common_types::SubstateRequirement;
use tari_ootle_transaction::{Network, Transaction, args};
use tari_template_lib_types::constants::{
    TARI_TOKEN,
    XTR_FAUCET_CLAIM_RESOURCE_ADDRESS,
    XTR_FAUCET_COMPONENT_ADDRESS,
    XTR_FAUCET_VAULT_ADDRESS,
};

pub fn builder(network: Network) -> impl Fn(u64) -> Transaction {
    move |_: u64| -> Transaction {
        let (signer_secret_key, signer_public_key) = RistrettoPublicKey::random_keypair(&mut OsRng);
        let signer_public_key = signer_public_key.to_byte_type();

        Transaction::builder(network.as_byte())
            .with_fee_instructions_builder(|builder| {
                builder
                    .create_account(signer_public_key)
                    .put_last_instruction_output_on_workspace("account")
                    .call_method(XTR_FAUCET_COMPONENT_ADDRESS, "take", args![Workspace("account")])
                    .call_method("account", "pay_fee", args![1000])
            })
            .with_inputs([
                SubstateRequirement::unversioned(TARI_TOKEN),
                SubstateRequirement::unversioned(XTR_FAUCET_COMPONENT_ADDRESS),
                SubstateRequirement::unversioned(XTR_FAUCET_VAULT_ADDRESS),
                SubstateRequirement::unversioned(XTR_FAUCET_CLAIM_RESOURCE_ADDRESS),
            ])
            .build_and_seal(&signer_secret_key)
    }
}
