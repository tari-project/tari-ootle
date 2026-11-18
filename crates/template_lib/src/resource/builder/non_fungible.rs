//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
use minicbor::Encode;
use tari_bor::to_value;
use tari_template_abi::rust::{collections::BTreeMap, prelude::*};
use tari_template_lib_types::{
    ComponentAddress,
    FunctionName,
    Metadata,
    NonFungibleId,
    ResourceAddress,
    ResourceType,
    access_rules::{ResourceAccessRules, UpdateRule},
    constants::{IMAGE_URL, TOKEN_SYMBOL},
};

use crate::{
    args::MintArg,
    error_variants::ERR_AUTH_HOOK_FN_NAME_LEN,
    models::{Bucket, ResourceAddressAllocation},
    resource::ResourceManager,
    types::{AccessRule, AuthHook, OwnerRule},
};

/// Utility for building non-fungible resources inside templates
pub struct NonFungibleResourceBuilder {
    owner_rule: OwnerRule,
    metadata: Metadata,
    access_rules: ResourceAccessRules,
    token_symbol: Option<String>,
    authorize_hook: Option<AuthHook>,
    address_allocation: Option<ResourceAddressAllocation>,
    is_total_supply_tracking_enabled: bool,
}

impl NonFungibleResourceBuilder {
    /// Returns a new non-fungible resource builder
    pub(super) fn new() -> Self {
        Self {
            owner_rule: OwnerRule::default(),
            metadata: Metadata::new(),
            access_rules: ResourceAccessRules::new(),
            token_symbol: None,
            authorize_hook: None,
            address_allocation: None,
            is_total_supply_tracking_enabled: true,
        }
    }

    /// Allows for chaining of builder methods even when conditionally applying builder methods.
    ///
    /// ## Example
    ///
    /// ```ignore
    /// use tari_template_lib::prelude::*;
    /// let resource = ResourceBuilder::non_fungible()
    ///    .with_owner_rule(rule!(allow_all))
    ///   .then(|builder| {
    ///     if some_condition {
    ///        builder.do_something_on_some_condition(..)
    ///     } else {
    ///        // or do nothing
    ///        builder
    ///     }
    ///   })
    ///   .build();
    /// ```
    pub fn then<F: FnOnce(Self) -> Self>(self, f: F) -> Self {
        f(self)
    }

    /// Sets up who will be the owner of the resource.
    /// Resource owners are the only ones allowed to update the resource's access rules after creation
    pub fn with_owner_rule(mut self, rule: OwnerRule) -> Self {
        self.owner_rule = rule;
        self
    }

    /// Sets up who can access the resource for each type of action
    pub fn with_access_rules(mut self, rules: ResourceAccessRules) -> Self {
        self.access_rules = rules;
        self
    }

    /// Sets the already allocated address for the resource
    pub fn with_address_allocation(self, address: ResourceAddressAllocation) -> Self {
        self.with_address_allocation_opt(Some(address))
    }

    /// Sets the already allocated address for the resource, optionally
    pub fn with_address_allocation_opt(mut self, address: Option<ResourceAddressAllocation>) -> Self {
        self.address_allocation = address;
        self
    }

    /// Sets up who can mint new tokens of the resource, and who may later change the mint rule.
    pub fn mintable<U: Into<UpdateRule>>(mut self, rule: AccessRule, updater: U) -> Self {
        self.access_rules = self.access_rules.mintable(rule, updater);
        self
    }

    /// Sets up who can burn (destroy) tokens of the resource, and who may later change the burn rule.
    pub fn burnable<U: Into<UpdateRule>>(mut self, rule: AccessRule, updater: U) -> Self {
        self.access_rules = self.access_rules.burnable(rule, updater);
        self
    }

    /// Sets up who can recall tokens of the resource, and who may later change the recall rule.
    /// A recall is the forceful withdrawal of tokens from any external vault.
    pub fn recallable<U: Into<UpdateRule>>(mut self, rule: AccessRule, updater: U) -> Self {
        self.access_rules = self.access_rules.recallable(rule, updater);
        self
    }

    /// Sets up who can freeze vaults containing this resource, and who may later change the freeze rule.
    pub fn freezable<U: Into<UpdateRule>>(mut self, rule: AccessRule, updater: U) -> Self {
        self.access_rules = self.access_rules.freezable(rule, updater);
        self
    }

    /// Sets up who can withdraw tokens of the resource from any vault, and who may later change the
    /// withdraw rule.
    pub fn withdrawable<U: Into<UpdateRule>>(mut self, rule: AccessRule, updater: U) -> Self {
        self.access_rules = self.access_rules.withdrawable(rule, updater);
        self
    }

    /// Sets up who can deposit tokens of the resource into any vault, and who may later change the
    /// deposit rule.
    pub fn depositable<U: Into<UpdateRule>>(mut self, rule: AccessRule, updater: U) -> Self {
        self.access_rules = self.access_rules.depositable(rule, updater);
        self
    }

    /// Sets up who can update the mutable data of the tokens in the resource, and who may later
    /// change that rule. By default only the resource owner may update the data, so a component that
    /// mutates token data on behalf of other signers must be granted access here, e.g.
    /// `rule!(component(address))`.
    pub fn update_non_fungible_data<U: Into<UpdateRule>>(mut self, rule: AccessRule, updater: U) -> Self {
        self.access_rules = self.access_rules.update_non_fungible_data(rule, updater);
        self
    }

    /// Sets up who can update the resource's metadata, and who may later change that rule. The
    /// token symbol remains immutable once set.
    pub fn update_metadata<U: Into<UpdateRule>>(mut self, rule: AccessRule, updater: U) -> Self {
        self.access_rules = self.access_rules.update_metadata(rule, updater);
        self
    }

    /// Sets up the specified `symbol` as the token symbol in the metadata of the resource
    pub fn with_token_symbol<S: Into<String>>(mut self, symbol: S) -> Self {
        self.token_symbol = Some(symbol.into());
        self
    }

    /// Adds a new metadata entry to the resource
    pub fn add_metadata<K: Into<String>, V: tari_bor::Encode<()>>(mut self, key: K, value: V) -> Self {
        self.metadata.insert(key, &value);
        self
    }

    /// Adds a new metadata entry to the resource
    ///
    /// Allows you to add a key-value pair to the resource's metadata.
    /// This is an alias for `.add_metadata()`.
    ///
    /// # Notes
    ///
    /// `.metadata()` will override any existing metadata with the same key.
    ///
    /// # Examples
    /// ```rust, ignore
    /// use tari_template_lib::resource::builder::ResourceBuilder;
    /// ResourceBuilder::non_fungible()
    ///    .metadata("CharacterName", "Tari")
    ///    .metadata("CharacterType", "Mascot")
    ///    .metadata("CharacterLvl", "99")
    /// .build();
    /// ```
    pub fn metadata<K: Into<String>, V: tari_bor::Encode<()>>(self, key: K, value: V) -> Self {
        self.add_metadata(key, value)
    }

    /// Sets up all the metadata entries of the resource
    pub fn with_metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Sets up the image URL of the resource
    pub fn with_image_url(self, url: String) -> Self {
        self.add_metadata(IMAGE_URL, url)
    }

    /// Specify a hook method that will be called to authorize actions on the resource.
    /// The signature of the method must be `fn(action: ResourceAuthAction, caller: CallerContext)`.
    /// The method should panic to deny the action.
    /// The resource will fail to build if the component's template does not have a method with the specified signature.
    /// Hooks are only run when the resource is acted on by an external component.
    ///
    /// The hook runs in a sandboxed frame: it may read, emit events and (when declared `&mut self`) update its own
    /// component state, but it cannot write any other substate, act on any vault or resource, or call another
    /// component. The acting component's caller badges are in scope for the hook's own method rule, and the sandbox
    /// is what stops the hook spending them on the acting component's behalf.
    ///
    /// ## Examples
    ///
    /// Building a resource with a hook from within a component
    /// ```ignore
    /// use tari_template_lib::{caller_context::CallerContext, prelude::ResourceBuilder};
    /// ResourceBuilder::non_fungible()
    ///     .with_authorization_hook(CallerContext::current_component_address(), "my_hook")
    ///     .build();
    /// ```
    ///
    /// Building a resource with a hook in a static template function. The address is allocated beforehand.
    ///
    /// ```ignore
    /// use tari_template_lib::{caller_context::CallerContext, prelude::ResourceBuilder};
    /// let alloc = CallerContext::allocate_component_address();
    /// ResourceBuilder::non_fungible()
    ///     .with_authorization_hook(*alloc.address(), "my_hook")
    ///     .build();
    /// ```
    pub fn with_authorization_hook<T: TryInto<FunctionName>>(
        mut self,
        address: ComponentAddress,
        auth_callback: T,
    ) -> Self {
        self.authorize_hook = Some(AuthHook::new(
            address,
            auth_callback
                .try_into()
                .unwrap_or_else(|_| panic!("{}", ERR_AUTH_HOOK_FN_NAME_LEN)),
        ));
        self
    }

    /// Sets up who can install, replace or remove the resource's authorization hook after creation.
    ///
    /// A hook is [`LOCKED`](tari_template_lib_types::access_rules::LOCKED) by default: it binds for the life of
    /// the resource, and a hook that panics or denies unconditionally leaves every vault of the resource
    /// unspendable. Setting an updater buys the ability to repair or retire the hook, at the cost of letting
    /// whoever satisfies the updater change the rules that existing holders are relying on.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use tari_template_lib::{caller_context::CallerContext, prelude::{OWNER, ResourceBuilder}};
    /// ResourceBuilder::non_fungible()
    ///     .with_authorization_hook(CallerContext::current_component_address(), "my_hook")
    ///     .with_authorization_hook_updater(OWNER)
    ///     .build();
    /// ```
    pub fn with_authorization_hook_updater<U: Into<UpdateRule>>(mut self, updater: U) -> Self {
        self.access_rules = self.access_rules.set_auth_hook_updater(updater);
        self
    }

    /// Disables the tracking of total supply for the resource.
    ///
    /// This is useful for resources that do not need to track the total supply.
    /// Disabling total supply tracking can save on fees when minting/burning.
    pub fn disable_total_supply_tracking(mut self) -> Self {
        self.is_total_supply_tracking_enabled = false;
        self
    }

    /// Build the resource, returning the address
    pub fn build(self) -> ResourceAddress {
        let (address, _) = self.build_internal(None);
        address
    }

    /// Builds the resource with the given initial non-fungible token IDs and returns a bucket holding them.
    ///
    /// # Panics
    /// Panics if `initial_supply` contains duplicate `NonFungibleId` entries.
    pub fn initial_supply<I: IntoIterator<Item = NonFungibleId>>(self, initial_supply: I) -> Bucket {
        let mut tokens = BTreeMap::new();
        for id in initial_supply {
            if tokens.contains_key(&id) {
                panic!("initial_supply contains duplicate non-fungible ID {id}");
            }
            tokens.insert(id, (tari_bor::Value::Null, tari_bor::Value::Null));
        }
        let mint_arg = MintArg::NonFungible { tokens };

        let (_, bucket) = self.build_internal(Some(mint_arg));
        bucket.expect("[initial_supply] Bucket not returned from engine")
    }

    /// Builds the resource with the given initial non-fungible tokens and their immutable/mutable data, returning a
    /// bucket holding them.
    ///
    /// # Panics
    /// Panics if `initial_supply` contains duplicate `NonFungibleId` entries or if any token data fails to encode.
    pub fn initial_supply_with_data<'a, I, T, U>(self, initial_supply: I) -> Bucket
    where
        I: IntoIterator<Item = (NonFungibleId, (&'a T, &'a U))>,
        T: Encode<()> + ?Sized + 'a,
        U: Encode<()> + ?Sized + 'a,
    {
        let mut tokens = BTreeMap::new();
        for (id, (data, mutable)) in initial_supply {
            if tokens.contains_key(&id) {
                panic!("initial_supply contains duplicate non-fungible ID {id}");
            }
            let value = (
                to_value(data).expect("failed to encode immutable NFT data"),
                to_value(mutable).expect("failed to encode mutable NFT data"),
            );
            tokens.insert(id, value);
        }
        let mint_arg = MintArg::NonFungible { tokens };

        let (_, bucket) = self.build_internal(Some(mint_arg));
        bucket.expect("[initial_supply] Bucket not returned from engine")
    }

    fn build_internal(mut self, mint_arg: Option<MintArg>) -> (ResourceAddress, Option<Bucket>) {
        if let Some(symbol) = self.token_symbol {
            self.metadata.insert(TOKEN_SYMBOL, &symbol);
        }

        ResourceManager::create(
            ResourceType::NonFungible,
            self.owner_rule,
            self.access_rules,
            self.metadata,
            mint_arg,
            None,
            self.authorize_hook,
            self.address_allocation,
            0,
            self.is_total_supply_tracking_enabled,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::builder::ResourceBuilder;

    #[test]
    #[should_panic(expected = "initial_supply contains duplicate non-fungible ID u64_1")]
    fn initial_supply_rejects_duplicate_ids() {
        let id = NonFungibleId::from_u64(1);
        ResourceBuilder::non_fungible().initial_supply([id.clone(), id]);
    }

    #[test]
    #[should_panic(expected = "initial_supply contains duplicate non-fungible ID u64_1")]
    fn initial_supply_with_data_rejects_duplicate_ids() {
        let id = NonFungibleId::from_u64(1);
        let tokens = [(id.clone(), (&1u32, &2u32)), (id, (&3u32, &4u32))];
        ResourceBuilder::non_fungible().initial_supply_with_data(tokens);
    }
}
