//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use tari_ootle_common_types::{SubstateRequirement, engine_types::substate::SubstateId};
use tari_ootle_transaction::{TransactionBuilder, UnsignedTransaction, builder::named_args::NamedArg};
use tari_template_lib_types::{
    Amount,
    ComponentAddress,
    FunctionName,
    ResourceAddress,
    TemplateAddress,
    constants::TARI_TOKEN,
};

use crate::{
    Address,
    ToAccountAddress,
    builtin_templates::traits::UnsignedTransactionBuilder,
    provider::{Provider, ProviderError, WantInput},
};

pub type IComponent<'a, P> = ComponentInvokeBuilder<'a, P>;

/// Marker for an ootle template interface (template-level functions, e.g. constructors).
/// Used as a type parameter in macro-generated structs to scope which methods are callable.
pub struct TemplateInterface {
    pub template: TemplateAddress,
}

/// Marker for an ootle component interface (instance methods with `&self` / `&mut self`).
/// Used as a type parameter in macro-generated structs to scope which methods are callable.
pub struct ComponentInterface {
    pub component: ComponentAddress,
}

/// Trait for extracting the inner builder and want list from any ootle builder.
///
/// This enables [`OotleInvoke::chain`] to merge instructions from one builder into another,
/// allowing cross-template transaction composition.
pub trait IntoBuildParts {
    fn into_build_parts(self) -> (TransactionBuilder, HashSet<WantInput>);
}

/// Shared builder operations available on both macro-generated template structs and
/// [`ComponentInvokeBuilder`]. All methods return `Self` for fluent chaining.
pub trait OotleInvoke: Sized {
    /// Pay fees from the default signer's account.
    fn pay_fee<A: Into<Amount>>(self, amount: A) -> Self;

    /// Explicitly request a vault for a specific resource in a component as an input.
    fn want_vault_for(
        self,
        component_address: ComponentAddress,
        resource_address: ResourceAddress,
        required: bool,
    ) -> Self;

    /// Explicitly request a specific substate as an input.
    fn want_substate(self, substate_id: SubstateId, required: bool) -> Self;

    /// Explicitly request all vaults from a component as inputs.
    fn want_all_vaults(self, component_address: ComponentAddress) -> Self;

    /// Save the last instruction's output on the workspace for use by subsequent instructions.
    fn put_last_instruction_output_on_workspace<T: Into<String>>(self, label: T) -> Self;

    /// Add a specific substate requirement as an input.
    fn add_input<S: Into<SubstateRequirement>>(self, substate_id: S) -> Self;

    /// Escape hatch to the raw [`TransactionBuilder`] for advanced usage.
    fn then<F: FnOnce(TransactionBuilder) -> TransactionBuilder>(self, f: F) -> Self;

    /// Merge instructions from another builder into this one.
    ///
    /// This enables cross-template transaction composition. The other builder's
    /// instructions are appended (with workspace IDs remapped to avoid collisions)
    /// and its want list is merged.
    ///
    /// Note: workspace references do NOT cross `chain` boundaries. If you need to
    /// reference a workspace item from the outer builder inside the chained builder,
    /// use [`OotleInvoke::then`] instead.
    fn chain<B: IntoBuildParts>(self, other: B) -> Self;
}

pub struct ComponentInvokeBuilder<'a, P> {
    builder: TransactionBuilder,
    provider: &'a P,
    want_list: HashSet<WantInput>,
}

impl<'a, P: Provider> UnsignedTransactionBuilder for ComponentInvokeBuilder<'a, P> {
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

impl<'a, P: Provider> IntoBuildParts for ComponentInvokeBuilder<'a, P> {
    fn into_build_parts(self) -> (TransactionBuilder, HashSet<WantInput>) {
        (self.builder, self.want_list)
    }
}

impl<'a, P: Provider> OotleInvoke for ComponentInvokeBuilder<'a, P> {
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

impl<'a, P: Provider> ComponentInvokeBuilder<'a, P> {
    pub fn new(provider: &'a P) -> Self {
        let network = provider.network();
        Self {
            builder: TransactionBuilder::new(network).with_auto_fill_inputs(),
            provider,
            want_list: HashSet::new(),
        }
    }

    /// Call a method on a component. Automatically adds [`WantInput::AllComponentVaults`]
    /// for the target component so its internal vaults are available as inputs.
    pub fn call_method<T>(mut self, component: ComponentAddress, method: T, args: Vec<NamedArg>) -> Self
    where
        T: TryInto<FunctionName>,
        <T as TryInto<FunctionName>>::Error: std::fmt::Debug,
    {
        self.want_list.insert(WantInput::AllComponentVaults {
            component_address: component,
        });
        self.builder = self.builder.call_method(component, method, args);
        self
    }

    /// Call a method without automatically discovering component vaults.
    /// Use this for workspace references or when you want full control over inputs.
    pub fn call_method_raw<C, T>(mut self, component: C, method: T, args: Vec<NamedArg>) -> Self
    where
        C: Into<tari_ootle_transaction::builder::NamedComponentCall>,
        T: TryInto<FunctionName>,
        <T as TryInto<FunctionName>>::Error: std::fmt::Debug,
    {
        self.builder = self.builder.call_method(component, method, args);
        self
    }

    /// Call a function (constructor or static method) on a template.
    pub fn call_function<T>(mut self, template_address: TemplateAddress, function: T, args: Vec<NamedArg>) -> Self
    where
        T: TryInto<FunctionName>,
        <T as TryInto<FunctionName>>::Error: std::fmt::Debug,
    {
        self.builder = self.builder.call_function(template_address, function, args);
        self
    }

    /// Returns a reference to the current want list (for testing/inspection).
    pub fn want_list(&self) -> &HashSet<WantInput> {
        &self.want_list
    }
}
