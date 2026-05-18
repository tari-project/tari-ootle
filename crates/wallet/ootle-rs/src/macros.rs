//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[macro_export]
macro_rules! resource_address {
    ($s:expr) => {
        $crate::macros::_macro_exports::ResourceAddress::from_hex($s).expect("Failed to parse resource string")
    };
}

pub mod _macro_exports {
    pub use tari_ootle_common_types::{SubstateRequirement, engine_types::substate::SubstateId};
    pub use tari_ootle_transaction::{
        self as transaction,
        TransactionBuilder,
        UnsignedTransaction,
        builder::named_args::{IntoArg, NamedArg},
    };
    pub use tari_template_lib_types::{Amount, ComponentAddress, ResourceAddress, TemplateAddress};

    pub use crate::{
        builtin_templates::{
            UnsignedTransactionBuilder,
            component::{
                ComponentInterface,
                ComponentInvokeBuilder,
                IntoBuildParts,
                TemplateInterface,
                TransactionBuildable,
            },
        },
        provider::{Provider, ProviderError, WantInput},
    };
}

/// Define a typed interface for invoking functions and methods on a Tari Ootle template.
///
/// This macro generates a single struct `{Name}<'a, P, I>` parameterized by an interface marker:
/// - **`{Name}<'a, P, ComponentInterface>`** — component methods (`&self` / `&mut self`)
/// - **`{Name}<'a, P, TemplateInterface>`** — template functions (no `self`, e.g. constructors)
///
/// Constructors:
/// - `{Name}::for_component(addr, &provider)` → component interface
/// - `{Name}::for_template(addr, &provider)` → template interface
///
/// Each generated method returns a
/// [`ComponentInvokeBuilder`](crate::builtin_templates::component::ComponentInvokeBuilder) that you chain with
/// `.pay_fee()`, `.want_vault_for()`, `.prepare()`, etc.
///
/// # Syntax
///
/// ```ignore
/// ootle_template! {
///     template MyTemplate {
///         // Template function (no self) — only callable on TemplateInterface
///         fn instantiate(initial_supply: Amount);
///
///         // Component methods (with self) — only callable on ComponentInterface
///         fn increase_supply(&mut self, amount: Amount);
///         fn get_balance(&self) -> Amount;
///     }
/// }
/// ```
///
/// # Example
///
/// ```ignore
/// use ootle_rs::ootle_template;
/// use tari_template_lib_types::Amount;
///
/// ootle_template! {
///     template StableCoin {
///         fn instantiate(view_key: RistrettoPublicKeyBytes);
///         fn increase_supply(&mut self, amount: Amount);
///     }
/// }
///
/// // Call a template function (e.g. constructor)
/// let tpl = StableCoin::for_template(template_addr, &provider);
/// let tx = tpl.instantiate(view_key).pay_fee(1000).prepare().await?;
///
/// // Call a component method
/// let coin = StableCoin::for_component(component_addr, &provider);
/// let tx = coin.increase_supply(Amount::new(1_000_000)).pay_fee(1000).prepare().await?;
/// ```
#[macro_export]
macro_rules! ootle_template {
    (
        template $name:ident {
            $($item:tt)*
        }
    ) => {
        $crate::__ootle_template_inner!(@parse $name [] [] $($item)*);
    };
}

/// Internal helper macro for `ootle_template!`. Parses function items and separates them into
/// template functions (no self) and component methods (&self / &mut self).
#[macro_export]
#[doc(hidden)]
macro_rules! __ootle_template_inner {
    // Parse a component method: fn name(&self, ...) -> ...;
    (@parse $name:ident
        [$($tpl:tt)*]
        [$($cmp:tt)*]
        fn $method:ident(& self $(, $param:ident: $ptype:ty)*) $(-> $ret:ty)?;
        $($rest:tt)*
    ) => {
        $crate::__ootle_template_inner!(@parse $name
            [$($tpl)*]
            [$($cmp)* { $method ($($param: $ptype),*) }]
            $($rest)*
        );
    };

    // Parse a component method: fn name(&mut self, ...) -> ...;
    (@parse $name:ident
        [$($tpl:tt)*]
        [$($cmp:tt)*]
        fn $method:ident(&mut self $(, $param:ident: $ptype:ty)*) $(-> $ret:ty)?;
        $($rest:tt)*
    ) => {
        $crate::__ootle_template_inner!(@parse $name
            [$($tpl)*]
            [$($cmp)* { $method ($($param: $ptype),*) }]
            $($rest)*
        );
    };

    // Parse a template function: fn name(...) -> ...;  (no self)
    (@parse $name:ident
        [$($tpl:tt)*]
        [$($cmp:tt)*]
        fn $func:ident($($param:ident: $ptype:ty),* $(,)?) $(-> $ret:ty)?;
        $($rest:tt)*
    ) => {
        $crate::__ootle_template_inner!(@parse $name
            [$($tpl)* { $func ($($param: $ptype),*) }]
            [$($cmp)*]
            $($rest)*
        );
    };

    // Base case: all items parsed, emit the struct and impl blocks
    (@parse $name:ident
        [$({ $func:ident ($($fp:ident: $ft:ty),*) })*]
        [$({ $method:ident ($($mp:ident: $mt:ty),*) })*]
    ) => {
        pub struct $name<'a, P, I> {
            interface: I,
            builder: $crate::macros::_macro_exports::ComponentInvokeBuilder<'a, P>,
        }

        // --- ComponentInterface: component methods ---
        #[allow(dead_code)]
        impl<'a, P: $crate::macros::_macro_exports::Provider>
            $name<'a, P, $crate::macros::_macro_exports::ComponentInterface>
        {
            pub fn for_component(
                component: $crate::macros::_macro_exports::ComponentAddress,
                provider: &'a P,
            ) -> Self {
                Self {
                    interface: $crate::macros::_macro_exports::ComponentInterface { component },
                    builder: $crate::macros::_macro_exports::ComponentInvokeBuilder::new(provider),
                }
            }

            pub fn component_address(&self) -> $crate::macros::_macro_exports::ComponentAddress {
                self.interface.component
            }

            $(
                pub fn $method(
                    self,
                    $($mp: impl $crate::macros::_macro_exports::IntoArg),*
                ) -> Self {
                    let Self { interface, builder } = self;
                    Self {
                        builder: builder.call_method(
                            interface.component,
                            stringify!($method),
                            vec![$($crate::macros::_macro_exports::IntoArg::into_arg($mp)),*],
                        ),
                        interface,
                    }
                }
            )*
        }

        impl<'a, P: $crate::macros::_macro_exports::Provider>
            $crate::macros::_macro_exports::IntoBuildParts
            for $name<'a, P, $crate::macros::_macro_exports::ComponentInterface>
        {
            fn into_build_parts(self) -> (
                $crate::macros::_macro_exports::TransactionBuilder,
                std::collections::HashSet<$crate::macros::_macro_exports::WantInput>,
            ) {
                $crate::macros::_macro_exports::IntoBuildParts::into_build_parts(self.builder)
            }
        }

        impl<'a, P: $crate::macros::_macro_exports::Provider>
            $crate::macros::_macro_exports::TransactionBuildable
            for $name<'a, P, $crate::macros::_macro_exports::ComponentInterface>
        {
            $crate::__ootle_invoke_impl!();
        }

        impl<'a, P: $crate::macros::_macro_exports::Provider>
            $crate::macros::_macro_exports::UnsignedTransactionBuilder
            for $name<'a, P, $crate::macros::_macro_exports::ComponentInterface>
        {
            $crate::__ootle_unsigned_tx_builder_impl!();
        }

        // --- TemplateInterface: template functions ---
        #[allow(dead_code)]
        impl<'a, P: $crate::macros::_macro_exports::Provider>
            $name<'a, P, $crate::macros::_macro_exports::TemplateInterface>
        {
            pub fn for_template(
                template: $crate::macros::_macro_exports::TemplateAddress,
                provider: &'a P,
            ) -> Self {
                Self {
                    interface: $crate::macros::_macro_exports::TemplateInterface { template },
                    builder: $crate::macros::_macro_exports::ComponentInvokeBuilder::new(provider),
                }
            }

            pub fn template_address(&self) -> $crate::macros::_macro_exports::TemplateAddress {
                self.interface.template
            }

            $(
                pub fn $func(
                    self,
                    $($fp: impl $crate::macros::_macro_exports::IntoArg),*
                ) -> Self {
                    let Self { interface, builder } = self;
                    Self {
                        builder: builder.call_function(
                            interface.template,
                            stringify!($func),
                            vec![$($crate::macros::_macro_exports::IntoArg::into_arg($fp)),*],
                        ),
                        interface,
                    }
                }
            )*
        }

        impl<'a, P: $crate::macros::_macro_exports::Provider>
            $crate::macros::_macro_exports::IntoBuildParts
            for $name<'a, P, $crate::macros::_macro_exports::TemplateInterface>
        {
            fn into_build_parts(self) -> (
                $crate::macros::_macro_exports::TransactionBuilder,
                std::collections::HashSet<$crate::macros::_macro_exports::WantInput>,
            ) {
                $crate::macros::_macro_exports::IntoBuildParts::into_build_parts(self.builder)
            }
        }

        impl<'a, P: $crate::macros::_macro_exports::Provider>
            $crate::macros::_macro_exports::TransactionBuildable
            for $name<'a, P, $crate::macros::_macro_exports::TemplateInterface>
        {
            $crate::__ootle_invoke_impl!();
        }

        impl<'a, P: $crate::macros::_macro_exports::Provider>
            $crate::macros::_macro_exports::UnsignedTransactionBuilder
            for $name<'a, P, $crate::macros::_macro_exports::TemplateInterface>
        {
            $crate::__ootle_unsigned_tx_builder_impl!();
        }
    };
}

/// Shared OotleInvoke trait implementation body. Used by `ootle_template!` generated types.
#[macro_export]
#[doc(hidden)]
macro_rules! __ootle_invoke_impl {
    () => {
        fn pay_fee<A: Into<$crate::macros::_macro_exports::Amount>>(self, amount: A) -> Self {
            let Self { interface, builder } = self;
            Self {
                builder: $crate::macros::_macro_exports::TransactionBuildable::pay_fee(builder, amount),
                interface,
            }
        }

        fn want_vault_for(
            self,
            component_address: $crate::macros::_macro_exports::ComponentAddress,
            resource_address: $crate::macros::_macro_exports::ResourceAddress,
            required: bool,
        ) -> Self {
            let Self { interface, builder } = self;
            Self {
                builder: $crate::macros::_macro_exports::TransactionBuildable::want_vault_for(
                    builder,
                    component_address,
                    resource_address,
                    required,
                ),
                interface,
            }
        }

        fn want_substate(self, substate_id: $crate::macros::_macro_exports::SubstateId, required: bool) -> Self {
            let Self { interface, builder } = self;
            Self {
                builder: $crate::macros::_macro_exports::TransactionBuildable::want_substate(
                    builder,
                    substate_id,
                    required,
                ),
                interface,
            }
        }

        fn want_all_vaults(self, component_address: $crate::macros::_macro_exports::ComponentAddress) -> Self {
            let Self { interface, builder } = self;
            Self {
                builder: $crate::macros::_macro_exports::TransactionBuildable::want_all_vaults(
                    builder,
                    component_address,
                ),
                interface,
            }
        }

        fn put_last_instruction_output_on_workspace<T: Into<String>>(self, label: T) -> Self {
            let Self { interface, builder } = self;
            Self {
                builder: $crate::macros::_macro_exports::TransactionBuildable::put_last_instruction_output_on_workspace(
                    builder, label,
                ),
                interface,
            }
        }

        fn add_input<S: Into<$crate::macros::_macro_exports::SubstateRequirement>>(self, substate_id: S) -> Self {
            let Self { interface, builder } = self;
            Self {
                builder: $crate::macros::_macro_exports::TransactionBuildable::add_input(builder, substate_id),
                interface,
            }
        }

        fn then<
            F: FnOnce(
                $crate::macros::_macro_exports::TransactionBuilder,
            ) -> $crate::macros::_macro_exports::TransactionBuilder,
        >(
            self,
            f: F,
        ) -> Self {
            let Self { interface, builder } = self;
            Self {
                builder: $crate::macros::_macro_exports::TransactionBuildable::then(builder, f),
                interface,
            }
        }

        fn chain<B: $crate::macros::_macro_exports::IntoBuildParts>(self, other: B) -> Self {
            let Self { interface, builder } = self;
            Self {
                builder: $crate::macros::_macro_exports::TransactionBuildable::chain(builder, other),
                interface,
            }
        }
    };
}

/// Shared UnsignedTransactionBuilder implementation body. Used by `ootle_template!` generated types.
#[macro_export]
#[doc(hidden)]
macro_rules! __ootle_unsigned_tx_builder_impl {
    () => {
        fn default_signer_address(&self) -> &$crate::Address {
            $crate::macros::_macro_exports::UnsignedTransactionBuilder::default_signer_address(&self.builder)
        }

        fn add_input<S: Into<$crate::macros::_macro_exports::SubstateRequirement>>(self, substate_id: S) -> Self {
            $crate::macros::_macro_exports::TransactionBuildable::add_input(self, substate_id)
        }

        async fn prepare(
            self,
        ) -> Result<$crate::macros::_macro_exports::UnsignedTransaction, $crate::macros::_macro_exports::ProviderError>
        {
            $crate::macros::_macro_exports::UnsignedTransactionBuilder::prepare(self.builder).await
        }
    };
}

/// A macro to create a `NonZeroU64` constant from a literal expression.
/// Panics at compile time if the value is zero.
#[macro_export]
macro_rules! const_nonzero_u64 {
    ($val:expr) => {{
        const __NONZERO: core::num::NonZeroU64 = core::num::NonZeroU64::new($val).expect("Value must be non-zero");
        __NONZERO
    }};
}

#[cfg(test)]
mod tests {
    use tari_template_lib_types::Amount;

    use crate::{Network, builtin_templates::component::TransactionBuildable};

    #[test]
    fn it_generates_a_non_zero() {
        // let nz = const_nonzero_u64!(5-5); // This line would not compile
        const NZ: core::num::NonZeroU64 = const_nonzero_u64!(5);
        assert_eq!(NZ.get(), 5);
    }

    // Verify that the macro expands and type-checks correctly.
    // This generates TestStableCoin<'a, P, I> where I is either
    // ComponentInterface (for methods) or TemplateInterface (for functions).
    ootle_template! {
        template TestStableCoin {
            fn instantiate(initial_supply: Amount);
            fn increase_supply(&mut self, amount: Amount);
            fn decrease_supply(&mut self, amount: Amount);
            fn withdraw(&mut self, amount: Amount);
            fn deposit(&mut self);
            fn get_balance(&self);
        }
    }

    mod mock_provider {
        use std::{
            collections::{HashMap, HashSet},
            sync::Weak,
        };

        use tari_ootle_common_types::engine_types::substate::{Substate, SubstateId};
        use tari_ootle_transaction::UnsignedTransaction;

        use crate::{
            Address,
            Network,
            provider::{Provider, ProviderResult, WantInput},
        };

        pub struct MockProvider {
            pub address: Address,
        }

        impl Provider for MockProvider {
            type Client = ();

            fn network(&self) -> Network {
                Network::LocalNet
            }

            fn weak_client(&self) -> Weak<Self::Client> {
                Weak::new()
            }

            fn default_signer_address(&self) -> &Address {
                &self.address
            }

            async fn resolve_input_want_list(
                &self,
                transaction: UnsignedTransaction,
                _want_list: &HashSet<WantInput>,
            ) -> ProviderResult<UnsignedTransaction> {
                Ok(transaction)
            }

            async fn fetch_substates<I: IntoIterator<Item = SubstateId> + Send>(
                &self,
                _substate_ids: I,
            ) -> ProviderResult<HashMap<SubstateId, Substate>> {
                Ok(HashMap::new())
            }
        }
    }

    #[test]
    fn ootle_template_macro_generates_component_methods() {
        use crate::keys::OotleSecretKey;

        let secret = OotleSecretKey::random(Network::LocalNet);
        let provider = mock_provider::MockProvider {
            address: secret.to_address(),
        };

        let component = tari_template_lib_types::ComponentAddress::new([0u8; 32].into());
        let coin = TestStableCoin::for_component(component, &provider);

        // Verify component_address accessor
        assert_eq!(coin.component_address(), component);

        // Verify typed methods return Self and can be chained
        let coin = TestStableCoin::for_component(component, &provider);
        let coin = coin.increase_supply(Amount::new(1000));
        // Can chain another typed method — this is the key improvement
        let coin = coin.decrease_supply(Amount::new(500));
        // Can chain shared builder methods via OotleInvoke trait
        let _coin = coin.pay_fee(1000u64);
    }

    ootle_template! {
        template TestAccount {
            fn deposit(&mut self);
            fn withdraw(&mut self, amount: Amount);
        }
    }

    #[test]
    fn ootle_template_chain_across_templates() {
        use crate::keys::OotleSecretKey;

        let secret = OotleSecretKey::random(Network::LocalNet);
        let provider = mock_provider::MockProvider {
            address: secret.to_address(),
        };

        let component_a = tari_template_lib_types::ComponentAddress::new([0u8; 32].into());
        let component_b = tari_template_lib_types::ComponentAddress::new([1u8; 32].into());

        // Chain: StableCoin.withdraw -> put on workspace -> Account.deposit (via then, since
        // workspace refs don't cross chain boundaries) -> chain an independent Account.withdraw
        let coin = TestStableCoin::for_component(component_a, &provider);
        let _coin = coin
            .withdraw(Amount::new(1000))
            .put_last_instruction_output_on_workspace("bucket")
            .chain(TestAccount::for_component(component_b, &provider).withdraw(Amount::new(500)))
            .pay_fee(1000u64);
    }

    #[test]
    fn ootle_template_macro_generates_template_functions() {
        use crate::keys::OotleSecretKey;

        let secret = OotleSecretKey::random(Network::LocalNet);
        let provider = mock_provider::MockProvider {
            address: secret.to_address(),
        };

        let template = tari_template_lib_types::TemplateAddress::from_array([1u8; 32]);
        let tpl = TestStableCoin::for_template(template, &provider);

        // Verify template_address accessor
        assert_eq!(tpl.template_address(), template);

        // Verify template function returns Self and can chain shared methods
        let tpl = TestStableCoin::for_template(template, &provider);
        let _tpl = tpl.instantiate(Amount::new(1_000_000)).pay_fee(1000u64);
    }
}
