//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use tari_ootle_common_types::SubstateRequirement;
use tari_ootle_transaction::{TransactionBuilder, UnsignedTransaction, args};
use tari_template_lib_types::{
    Amount,
    ComponentAddress,
    ResourceAddress,
    UtxoAddress,
    constants::{
        TARI_TOKEN,
        XTR_FAUCET_CLAIM_RESOURCE_ADDRESS,
        XTR_FAUCET_COMPONENT_ADDRESS,
        XTR_FAUCET_VAULT_ADDRESS,
    },
    stealth::StealthTransferStatement,
};

use crate::{
    Address,
    ToAccountAddress,
    builtin_templates::{
        UnsignedTransactionBuilder,
        component::{IntoBuildParts, TransactionBuildable},
    },
    macros::_macro_exports::SubstateId,
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

    pub fn into_stealth_transfer(mut self, transfer: StealthTransferStatement) -> Self {
        let amount = transfer.inputs_statement.revealed_amount;
        if !amount.is_positive() {
            panic!("Transfer amount must be positive");
        }
        let bucket_name = self.next_workspace_name();

        let Some(account_name) = self.account_workspace_name.as_ref() else {
            // TODO: make this panic impossible
            panic!(
                "Call take_faucet_funds() before converting to a stealth transfer to ensure the builder has the \
                 necessary workspace for revealed outputs and fee payment"
            );
        };

        // Request all UTXO inputs
        for input in &transfer.inputs_statement.inputs {
            self.want_list.insert(WantInput::SpecificSubstate {
                substate_id: UtxoAddress::new(TARI_TOKEN, input.commitment.into()).into(),
                required: true,
            });
        }

        self.builder = self.builder.with_fee_instructions_builder(|builder| {
            builder
                .call_method(account_name, "withdraw", args![TARI_TOKEN, amount])
                .put_last_instruction_output_on_workspace(&bucket_name)
                .stealth_transfer(TARI_TOKEN, transfer)
        });
        self
    }

    pub fn and_pay_fee_from_revealed_output(mut self) -> Self {
        let bucket_name = self.next_workspace_name();
        self.builder = self.builder.with_fee_instructions_builder(|builder| {
            builder
                .put_last_instruction_output_on_workspace(&bucket_name)
                .pay_fee_from_bucket(bucket_name)
        });
        self
    }

    /// Takes the fixed faucet amount (1,000 TARI) and deposits them into the default signer's account.
    pub fn take_faucet_funds(mut self) -> Self {
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
                // Create the recipient account if it doesn't exist
                .create_account(recipient_account_pk)
                .put_last_instruction_output_on_workspace(&account_name)
                .call_method(XTR_FAUCET_COMPONENT_ADDRESS, "take", args![Workspace(&account_name)])
                .add_input(XTR_FAUCET_VAULT_ADDRESS)
                .add_input(XTR_FAUCET_CLAIM_RESOURCE_ADDRESS)
        });
        self.account_workspace_name = Some(account_name);
        self
    }
}

impl<P: Provider> TransactionBuildable for FaucetInvokeBuilder<'_, P> {
    fn pay_fee<A: Into<Amount>>(mut self, amount: A) -> Self {
        let component_addr = self.provider.default_signer_address().to_account_address();
        self.want_list.insert(WantInput::VaultForResource {
            component_address: component_addr,
            resource_address: TARI_TOKEN,
            required: true,
        });
        self.builder = self.builder.pay_fee_from_component(component_addr, amount);
        self
    }

    fn want_vault_for(
        mut self,
        component_address: ComponentAddress,
        resource_address: ResourceAddress,
        required: bool,
    ) -> Self {
        self.want_list.insert(WantInput::VaultForResource {
            component_address,
            resource_address,
            required,
        });
        self
    }

    fn want_substate(mut self, substate_id: SubstateId, required: bool) -> Self {
        self.want_list
            .insert(WantInput::SpecificSubstate { substate_id, required });
        self
    }

    fn want_all_vaults(mut self, component_address: ComponentAddress) -> Self {
        self.want_list
            .insert(WantInput::AllComponentVaults { component_address });
        self
    }

    fn put_last_instruction_output_on_workspace<T: Into<String>>(mut self, label: T) -> Self {
        self.builder = self.builder.put_last_instruction_output_on_workspace(label);
        self
    }

    fn add_input<S: Into<SubstateRequirement>>(mut self, substate_id: S) -> Self {
        self.builder = self.builder.add_input(substate_id);
        self
    }

    fn then<F: FnOnce(TransactionBuilder) -> TransactionBuilder>(mut self, f: F) -> Self {
        self.builder = f(self.builder);
        self
    }

    fn chain<B: IntoBuildParts>(mut self, other: B) -> Self {
        let (other_builder, other_wants) = other.into_build_parts();
        self.builder = self.builder.merge(other_builder);
        self.want_list.extend(other_wants);
        self
    }
}
