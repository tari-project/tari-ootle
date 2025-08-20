//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib_types::Amount;

use super::{IMAGE_URL, TOKEN_SYMBOL};
use crate::{
    args::MintArg,
    auth::{AccessRule, AuthHook, OwnerRule, ResourceAccessRules},
    models::{Bucket, ComponentAddress, Metadata, ResourceAddress, ResourceAddressAllocation},
    resource::{ResourceManager, ResourceType, DEFAULT_DIVISIBILITY},
    types::crypto::RistrettoPublicKeyBytes,
};

/// Implements the builder pattern for Confidential resources.
pub struct StealthResourceBuilder {
    metadata: Metadata,
    access_rules: ResourceAccessRules,
    view_key: Option<RistrettoPublicKeyBytes>,
    token_symbol: Option<String>,
    owner_rule: OwnerRule,
    authorize_hook: Option<AuthHook>,
    address_allocation: Option<ResourceAddressAllocation>,
    divisibility: u8,
    is_total_supply_tracking_enabled: bool,
}

impl StealthResourceBuilder {
    /// Returns a new confidential resource builder
    pub(super) fn new() -> Self {
        Self {
            metadata: Metadata::new(),
            access_rules: ResourceAccessRules::new(),
            view_key: None,
            token_symbol: None,
            owner_rule: OwnerRule::default(),
            authorize_hook: None,
            address_allocation: None,
            divisibility: DEFAULT_DIVISIBILITY,
            is_total_supply_tracking_enabled: true,
        }
    }

    /// Allows for chaining of builder methods even when conditionally applying builder methods.
    ///
    /// ## Example
    ///
    /// ```ignore
    /// use tari_template_lib::prelude::*;
    /// let resource = ResourceBuilder::stealth()
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
    pub fn with_address_allocation(mut self, address: ResourceAddressAllocation) -> Self {
        self.address_allocation = Some(address);
        self
    }

    /// Specify a view key for the stealth resource. This allows anyone with the secret key to uncover the balance
    /// of commitments generated for the resource.
    /// NOTE: it is not currently possible to change the view key after the resource is created.
    /// Equivalent to calling `with_view_key_opt(Some(view_key))`.
    pub fn with_view_key(self, view_key: RistrettoPublicKeyBytes) -> Self {
        self.with_view_key_opt(Some(view_key))
    }

    /// Optionally, specify a view key for the stealth resource. This allows anyone with the secret key to uncover the
    /// balance of commitments generated for the resource.
    /// NOTE: it is not currently possible to change the view key after the resource is created.
    pub fn with_view_key_opt(mut self, view_key: Option<RistrettoPublicKeyBytes>) -> Self {
        self.view_key = view_key;
        self
    }

    /// Sets up who can mint new tokens of the resource
    pub fn mintable(mut self, rule: AccessRule) -> Self {
        self.access_rules = self.access_rules.mintable(rule);
        self
    }

    /// Sets up who can burn (destroy) tokens of the resource
    pub fn burnable(mut self, rule: AccessRule) -> Self {
        self.access_rules = self.access_rules.burnable(rule);
        self
    }

    /// Sets up who can recall tokens of the resource.
    /// A recall is the forceful withdrawal of tokens from any external vault
    pub fn recallable(mut self, rule: AccessRule) -> Self {
        self.access_rules = self.access_rules.recallable(rule);
        self
    }

    /// Sets up who can freeze vaults containing this resource.
    pub fn freezable(mut self, rule: AccessRule) -> Self {
        self.access_rules = self.access_rules.freezable(rule);
        self
    }

    /// Sets up who can withdraw tokens of the resource from any vault
    pub fn withdrawable(mut self, rule: AccessRule) -> Self {
        self.access_rules = self.access_rules.withdrawable(rule);
        self
    }

    /// Sets up who can deposit tokens of the resource into any vault
    pub fn depositable(mut self, rule: AccessRule) -> Self {
        self.access_rules = self.access_rules.depositable(rule);
        self
    }

    /// Sets up whom (apart from the owner) can update the access rules of the resource.
    pub fn update_access_rules(mut self, rule: AccessRule) -> Self {
        self.access_rules = self.access_rules.update_access_rules(rule);
        self
    }

    /// Sets up the specified `symbol` as the token symbol in the metadata of the resource
    pub fn with_token_symbol<S: Into<String>>(mut self, symbol: S) -> Self {
        self.token_symbol = Some(symbol.into());
        self
    }

    /// Adds a new metadata entry to the resource
    pub fn add_metadata<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.metadata.insert(key, value);
        self
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

    /// Sets the divisibility of the resource. i.e. the number of decimal places.
    /// Panic if the divisibility is greater than 18.
    pub fn with_divisibility(mut self, divisibility: u8) -> Self {
        if divisibility > 18 {
            panic!("Divisibility cannot be greater than 18");
        }
        self.divisibility = divisibility;
        self
    }

    /// Specify a hook method that will be called to authorize actions on the resource.
    /// The signature of the method must be `fn(action: ResourceAuthAction, caller: CallerContext)`.
    /// The method should panic to deny the action.
    /// The resource will fail to build if the component's template does not have a method with the correct signature.
    /// Hooks are only run when the resource is acted on by an external component.
    ///
    /// ## Examples
    ///
    /// Building a resource with a hook from within a component
    /// ```ignore
    /// # use tari_template_lib::{caller_context::CallerContext, prelude::ResourceBuilder};
    /// ResourceBuilder::confidential()
    ///     .with_authorization_hook(CallerContext::current_component_address(), "my_hook")
    ///     .build();
    /// ```
    ///
    /// Building a resource with a hook in a static template function. The address is allocated beforehand.
    ///
    /// ```ignore
    /// # use tari_template_lib::{caller_context::CallerContext, prelude::ResourceBuilder};
    /// let alloc = CallerContext::allocate_component_address();
    /// ResourceBuilder::confidential()
    ///     .with_authorization_hook(*alloc.address(), "my_hook")
    ///     .build();
    /// ```
    pub fn with_authorization_hook<T: Into<String>>(mut self, address: ComponentAddress, auth_callback: T) -> Self {
        self.authorize_hook = Some(AuthHook::new(address, auth_callback.into()));
        self
    }

    /// Disables the tracking of total supply for the resource.
    ///
    /// This is useful for resources that do not need to track the total supply.
    /// Disabling total supply tracking can save on fees.
    pub fn disable_total_supply_tracking(mut self) -> Self {
        self.is_total_supply_tracking_enabled = false;
        self
    }

    /// Build the resource, returning the address
    pub fn build(self) -> ResourceAddress {
        let (address, _) = self.build_internal(None);
        address
    }

    /// Sets up how many tokens are going to be minted on resource creation
    /// This builds the resource and mints the initial supply of tokens, returning the address of the resource.
    /// NOTE that stealth resources do not return the bucket of the initial supply since
    /// they are minted as individual UTXO substates and cannot be placed in vault.
    pub fn initial_supply(self, initial_supply: Amount) -> Bucket {
        let mint_arg = MintArg::Stealth { amount: initial_supply };

        let (_, bucket) = self.build_internal(Some(mint_arg));
        bucket.expect("[initial_supply] Bucket not returned from engine")
    }

    fn build_internal(mut self, mint_arg: Option<MintArg>) -> (ResourceAddress, Option<Bucket>) {
        if let Some(symbol) = self.token_symbol {
            self.metadata.insert(TOKEN_SYMBOL, symbol);
        }
        ResourceManager::create(
            ResourceType::Stealth,
            self.owner_rule,
            self.access_rules,
            self.metadata,
            mint_arg,
            self.view_key,
            self.authorize_hook,
            self.address_allocation,
            self.divisibility,
            self.is_total_supply_tracking_enabled,
        )
    }
}
