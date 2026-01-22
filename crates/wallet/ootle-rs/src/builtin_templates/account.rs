//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use tari_ootle_common_types::SubstateRequirement;
use tari_ootle_transaction::{args, TransactionBuilder, UnsignedTransaction};
use tari_template_lib_types::{Amount, ResourceAddress};

use crate::{
    builtin_templates::traits::InvokeBuilder,
    provider::{Provider, ProviderError, WantInput},
    Address,
    ToAccountAddress,
};

pub type IAccount<'a, P> = AccountInvokeBuilder<'a, P>;

pub struct AccountInvokeBuilder<'a, P> {
    builder: TransactionBuilder,
    provider: &'a P,
    want_list: HashSet<WantInput>,
}

impl<'a, P: Provider> InvokeBuilder for AccountInvokeBuilder<'a, P> {
    fn builder(&self) -> &TransactionBuilder {
        &self.builder
    }

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
        let unsigned_tx = builder.build_unsigned_transaction();
        let unsigned_tx = provider.resolve_input_want_list(unsigned_tx, &want_list).await?;
        Ok(unsigned_tx)
    }
}

impl<'a, P: Provider> AccountInvokeBuilder<'a, P> {
    pub fn new(provider: &'a P) -> Self {
        let network = provider.network();
        Self {
            builder: TransactionBuilder::new(network).with_auto_fill_inputs(),
            provider,
            want_list: HashSet::new(),
        }
    }

    pub fn pay_fee<A: Into<Amount>>(mut self, amount: A) -> Self {
        let component_addr = self.default_signer_address().to_account_address();
        self.builder = self.builder.pay_fee_from_component(component_addr, amount);
        self
    }

    fn next_bucket_name(&mut self) -> String {
        format!("__AccountInvokeBuilder_{}", self.builder.next_workspace_id())
    }

    pub fn public_transfer<A: Into<Amount>>(
        mut self,
        to: &Address,
        resource_address: ResourceAddress,
        amount: A,
    ) -> Self {
        let amount: Amount = amount.into();
        if !amount.is_positive() {
            panic!("Transfer amount must be positive");
        }
        let from_component_addr = self.default_signer_address().to_account_address();
        let to_component_addr = to.to_account_address();
        let bucket_name = self.next_bucket_name();
        // We need the vault to pay from
        self.want_list.insert(WantInput::VaultForResource {
            component_address: from_component_addr,
            resource_address,
            required: true,
        });
        // We need the substate of the recipient account to deposit into, if it exists
        self.want_list.insert(WantInput::SubstateIfExists {
            substate_id: to_component_addr.into(),
        });
        // We need the vault to deposit into, if it exists
        self.want_list.insert(WantInput::VaultForResource {
            component_address: to_component_addr,
            resource_address,
            // If it doesn't exist, the  CreateAccount instruction will create it
            required: false,
        });

        self.builder = self
            .builder
            .call_method(from_component_addr, "withdraw", args![resource_address, amount])
            .put_last_instruction_output_on_workspace(&bucket_name)
            .create_account_with_bucket(*to.account_public_key(), bucket_name);
        self
    }
}
