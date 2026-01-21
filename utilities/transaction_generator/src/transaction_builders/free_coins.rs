//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use rand::rngs::OsRng;
use tari_crypto::{keys::PublicKey, ristretto::RistrettoPublicKey};
use tari_engine_types::component::derive_component_address_from_public_key;
use tari_ootle_common_types::{Network, SubstateRequirement};
use tari_ootle_transaction::{args, Transaction};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::constants::{XTR, XTR_FAUCET_COMPONENT_ADDRESS, XTR_FAUCET_VAULT_ADDRESS};

pub fn builder(network: Network) -> impl Fn(u64) -> Transaction {
    move |_: u64| -> Transaction {
        let (signer_secret_key, signer_public_key) = RistrettoPublicKey::random_keypair(&mut OsRng);
        let signer_public_key = signer_public_key.to_byte_type();

        let account_address = derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &signer_public_key);

        Transaction::builder_localnet()
            .for_network(network.as_byte())
            .with_fee_instructions_builder(|builder| {
                builder
                    .call_method(XTR_FAUCET_COMPONENT_ADDRESS, "take", args![5000])
                    .put_last_instruction_output_on_workspace("free_coins")
                    .create_account_with_bucket(signer_public_key, "free_coins")
                    .call_method(account_address, "pay_fee", args![1000])
            })
            .with_inputs([
                SubstateRequirement::unversioned(XTR),
                SubstateRequirement::unversioned(XTR_FAUCET_COMPONENT_ADDRESS),
                SubstateRequirement::unversioned(XTR_FAUCET_VAULT_ADDRESS),
            ])
            .build_and_seal(&signer_secret_key)
    }
}
