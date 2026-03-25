//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use tari_ootle_common_types::SubstateRequirement;
use tari_ootle_transaction::{TransactionBuilder, UnsignedTransaction, args};
use tari_template_lib_types::{
    Amount,
    UtxoAddress,
    constants::{TARI, TARI_TOKEN, XTR_FAUCET_COMPONENT_ADDRESS, XTR_FAUCET_VAULT_ADDRESS},
    stealth::StealthTransferStatement,
};

use crate::{
    Address,
    ToAccountAddress,
    builtin_templates::UnsignedTransactionBuilder,
    provider::{Provider, ProviderError, WantInput},
};

pub type IFaucet<'a, P> = FaucetInvokeBuilder<'a, P>;

pub struct FaucetInvokeBuilder<'a, P> {
    builder: TransactionBuilder,
    provider: &'a P,
    want_list: HashSet<WantInput>,
    account_workspace_name: Option<String>,
}

impl<'a, P: Provider> UnsignedTransactionBuilder for FaucetInvokeBuilder<'a, P> {
    fn default_signer_address(&self) -> &Address {
        self.provider.default_signer_address()
    }

    fn add_input<S: Into<SubstateRequirement>>(mut self, substate_id: S) -> Self {
        self.builder = self.builder.add_input(substate_id);
        self
    }

    async fn prepare(self) -> Result<UnsignedTransaction, ProviderError> {
        let Self {
            builder,
            provider,
            want_list,
            ..
        } = self;
        let unsigned_tx = builder.build_unsigned();
        let unsigned_tx = provider.resolve_input_want_list(unsigned_tx, &want_list).await?;
        Ok(unsigned_tx)
    }
}

impl<'a, P: Provider> FaucetInvokeBuilder<'a, P> {
    pub fn new(provider: &'a P) -> Self {
        let network = provider.network();
        Self {
            builder: TransactionBuilder::new(network).with_auto_fill_inputs(),
            provider,
            want_list: HashSet::new(),
            account_workspace_name: None,
        }
    }

    pub fn pay_fee<A: Into<Amount>>(mut self, amount: A) -> Self {
        if let Some(name) = self.account_workspace_name.take() {
            self.builder = self.builder.pay_fee_from_component(name, amount);
        } else {
            let component_addr = self.default_signer_address().to_account_address();
            self.want_list.insert(WantInput::VaultForResource {
                component_address: component_addr,
                resource_address: TARI_TOKEN,
                required: true,
            });
            self.builder = self.builder.pay_fee_from_component(component_addr, amount);
        }
        self
    }

    fn next_workspace_name(&mut self) -> String {
        format!("__AccountInvokeBuilder_{}", self.builder.next_workspace_id())
    }

    /// Takes the maximum permitted funds from the faucet and deposits them into the default signer's account.
    pub fn take_max_faucet_funds(self) -> Self {
        // NOTE: that the actual maximum is currently 10_000, but we set it to 1_000 here to be conservative.
        const FAUCET_MAX_TAKE_AMOUNT: u64 = 1_000 * TARI;
        self.take_faucet_funds(FAUCET_MAX_TAKE_AMOUNT)
    }

    pub fn take_faucet_funds_stealth(
        mut self,
        transfer: StealthTransferStatement,
        pay_revealed_amount_as_fees: bool,
    ) -> Self {
        let amount = transfer.inputs_statement.revealed_amount;
        if !amount.is_positive() {
            panic!("Transfer amount must be positive");
        }

        let has_revealed_output = transfer.outputs_statement.revealed_output_amount.is_positive();
        let workspace_names = has_revealed_output.then(|| {
            (
                (!pay_revealed_amount_as_fees).then(|| self.next_workspace_name()),
                self.next_workspace_name(),
            )
        });
        let recipient_account_pk = *self.default_signer_address().account_public_key();

        // Request all UTXO inputs
        for input in &transfer.inputs_statement.inputs {
            self.want_list.insert(WantInput::SpecificSubstate {
                substate_id: UtxoAddress::new(TARI_TOKEN, input.commitment.into()).into(),
                required: true,
            });
        }

        self.builder = self.builder.with_fee_instructions_builder(|builder| {
            builder
                .add_input(XTR_FAUCET_VAULT_ADDRESS)
                .call_method(XTR_FAUCET_COMPONENT_ADDRESS, "take_confidential", args![transfer])
                .then(|builder| {
                    if let Some((account_ws_name, bucket_name)) = &workspace_names {
                        if let Some(account_ws_name) = account_ws_name {
                            builder
                                .put_last_instruction_output_on_workspace(bucket_name)
                                // Create the recipient account if it doesn't exist
                                .create_account_with_bucket(recipient_account_pk, bucket_name)
                                .put_last_instruction_output_on_workspace(account_ws_name)
                        } else {
                            builder
                                .put_last_instruction_output_on_workspace(bucket_name)
                                .pay_fee_from_bucket(bucket_name)
                        }
                    } else {
                        builder
                    }
                })
        });
        self.account_workspace_name = workspace_names.and_then(|(account_ws_name, _)| account_ws_name);
        self
    }

    /// Takes the specified amount of funds from the faucet and deposits them into the default signer's account.
    pub fn take_faucet_funds<A: Into<Amount>>(mut self, amount: A) -> Self {
        let amount: Amount = amount.into();
        if !amount.is_positive() {
            panic!("Transfer amount must be positive");
        }
        let bucket_name = self.next_workspace_name();
        let recipient_account_pk = *self.default_signer_address().account_public_key();
        let recipient_account_addr = self.default_signer_address().to_account_address();
        self.want_list.insert(WantInput::VaultForResource {
            component_address: recipient_account_addr,
            resource_address: TARI_TOKEN,
            //     If it doesn't exist, deposit will create it
            required: false,
        });
        self.want_list.insert(WantInput::SpecificSubstate {
            substate_id: recipient_account_addr.into(),
            required: false,
        });

        let account_name = self.next_workspace_name();
        self.builder = self.builder.with_fee_instructions_builder(|builder| {
            builder
                .call_method(XTR_FAUCET_COMPONENT_ADDRESS, "take", args![])
                .add_input(XTR_FAUCET_VAULT_ADDRESS)
                .put_last_instruction_output_on_workspace(&bucket_name)
                // Create the recipient account if it doesn't exist
                .create_account_with_bucket(recipient_account_pk, &bucket_name)
                .put_last_instruction_output_on_workspace(&account_name)
        });
        self.account_workspace_name = Some(account_name);
        self
    }
}
