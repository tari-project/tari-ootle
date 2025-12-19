//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use super::{IMAGE_URL, TOKEN_SYMBOL};
use crate::{
    args::MintArg,
    auth::{AccessRule, AuthHook, OwnerRule, ResourceAccessRules},
    models::{Bucket, ComponentAddress, Metadata, ResourceAddress, ResourceAddressAllocation},
    prelude::ConfidentialOutputStatement,
    resource::{ResourceManager, DEFAULT_DIVISIBILITY},
    types::{crypto::RistrettoPublicKeyBytes, ResourceType},
};

/// A builder for creating confidential fungible resources (tokens) inside templates.
///
/// This builder provides a fluent API to configure and create confidential fungible tokens with
/// various properties such as ownership rules, access controls, metadata, divisibility,
/// and authorization hooks.
///
/// If values are not set, defaults for the various properties will be used.
///
/// # Usage
///
/// You typically start by creating a new builder via [`ResourceBuilder::confidential()`],
/// then chain configuration methods like `.with_owner_rule()`, `.mintable()`, or
/// `.with_token_symbol()`, and finally call `.build()` or `.initial_supply()` to create
/// the resource.
///
/// # Note
/// 
/// The `ConfidentialResourceBuilder` requires you to provide a [`ConfidentialOutputStatement`] if you wish to set an initial supply. 
/// Additional supply can be minted later using the `mintable` access rule via another provided [`ConfidentialOutputStatement`]. Generation
/// of the `ConfidentialOutputStatement`] is non-trivial; examples of creation of the statement can be found in the test cases for the
/// `ConfidentialResourceBuilder`.
/// # Examples
///
/// Basic usage:
/// ```rust, ignore
/// use tari_template_lib::resource::builder::ResourceBuilder;
///
/// let resource_address = ResourceBuilder::confidential()
///     .with_token_symbol("CONF_TARI")
///     .with_divisibility(9)
///     .mintable(rule!(allow_all))
///     .build();
/// ```
///
/// Creating a confidential resource with an initial supply:
/// ```rust, ignore
/// use tari_template_lib::resource::builder::ResourceBuilder;
/// use tari_template_lib::prelude::*;
///
/// let proof = create_confidential_output_statement(...); // Omitted for brevity
/// let bucket = ResourceBuilder::confidential()
///     .with_token_symbol("CONF_TOKEN")
///     .initial_supply(proof);
/// ```
/// 
pub struct ConfidentialResourceBuilder {
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

impl ConfidentialResourceBuilder {
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
    /// let resource = ResourceBuilder::confidential()
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
    ///
    /// By default, the owner is the signer of the resource creation transaction ([`OwnerRule::OwnedBySigner`]).
    ///
    /// Resource owners are the only ones allowed to update the resource's access rules after creation.
    pub fn with_owner_rule(mut self, rule: OwnerRule) -> Self {
        self.owner_rule = rule;
        self
    }

    /// Sets up who can access the resource for each type of action
    ///
    /// This allows you to pass the access rules that will be applied to the resource in a single call.
    ///
    /// Using this function will override all previously set access rules.
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

    /// Disables the tracking of total supply for the resource.
    ///
    /// By default, total supply tracking is enabled. `.disable_total_supply_tracking()` can be used to disable it.
    /// Use cases include privacy focused tokens or utility tokens where the total supply is not relevant.
    ///
    /// # Examples
    /// ```rust, ignore
    /// use tari_template_lib::resource::builder::ResourceBuilder;
    /// ResourceBuilder::confidential()
    ///     .disable_total_supply_tracking()
    ///     .build();
    /// ```
    pub fn disable_total_supply_tracking(mut self) -> Self {
        self.is_total_supply_tracking_enabled = false;
        self
    }

    /// Specify a view key for the confidential resource. This allows anyone with the secret key to uncover the balance
    /// of commitments generated for the resource.
    /// 
    /// The view key is [`RistrettoPublicKeyBytes`] type, that is, the compressed public key representation of the secret view key.
    /// 
    /// # Note
    /// It is not currently possible to change the view key after the resource is created.
    pub fn with_view_key(self, view_key: RistrettoPublicKeyBytes) -> Self {
        self.with_view_key_opt(Some(view_key))
    }

    /// Optionally, specify a view key for the confidential resource. This allows anyone with the secret view key to uncover
    /// the balance of commitments generated for the resource.
    /// NOTE: it is not currently possible to change the view key after the resource is created.
    pub fn with_view_key_opt(mut self, view_key: Option<RistrettoPublicKeyBytes>) -> Self {
        self.view_key = view_key;
        self
    }

    /// Sets up who can mint new tokens of the resource
    ///
    /// Allows you to pass an [`AccessRule`] that defines who can mint new tokens of the resource.
    ///
    /// By default, minting is disabled for all users.
    ///
    /// #Examples
    ///
    /// ```rust, ignore
    /// use tari_template_lib::auth::AccessRule;
    /// use tari_template_lib::resource::builder::ResourceBuilder;
    /// ResourceBuilder::confidential()
    ///     .mintable(rule!(allow_all))
    ///     .build();
    /// ```
    pub fn mintable(mut self, rule: AccessRule) -> Self {
        self.access_rules = self.access_rules.mintable(rule);
        self
    }

    /// Sets up who can burn (destroy) tokens of the resource
    ///
    /// Allows you to pass an [`AccessRule`] that defines who can burn tokens of the resource.
    /// By default, burning is disabled for all users.
    ///
    /// #Examples
    ///
    /// ```rust, ignore
    /// use tari_template_lib::auth::AccessRule;
    /// use tari_template_lib::resource::builder::ResourceBuilder;
    /// ResourceBuilder::confidential()
    ///     .burnable(rule!(allow_all))
    ///     .build();
    /// ```
    pub fn burnable(mut self, rule: AccessRule) -> Self {
        self.access_rules = self.access_rules.burnable(rule);
        self
    }

    /// Sets up who can recall tokens of the resource.
    ///
    /// A recall is the forceful withdrawal of **ALL** tokens from any external vault containing the resource.
    ///
    /// Allows you to pass an [`AccessRule`] that defines who can recall tokens from a vault.
    ///
    /// By default, recalling is disabled for all users.
    ///
    /// # Examples
    ///
    /// ```rust, ignore
    /// use tari_template_lib::auth::AccessRule;
    /// use tari_template_lib::resource::builder::ResourceBuilder;
    /// ResourceBuilder::confidential()
    ///    .recallable(rule!(require(resource_admin_badge)))
    ///   .build();
    /// ```
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
    ///
    /// Allows you to pass an [`AccessRule`] that defines who can withdraw tokens (via a specified amount) from a vault.
    ///
    /// By default, withdrawal is allowed for all users.
    ///
    /// # Examples
    ///
    /// ```rust, ignore
    /// use tari_template_lib::auth::AccessRule;
    /// use tari_template_lib::resource::builder::ResourceBuilder;
    /// ResourceBuilder::confidential()
    ///     .withdrawable(AccessRule::DenyAll)
    ///     .build();
    /// ```
    pub fn withdrawable(mut self, rule: AccessRule) -> Self {
        self.access_rules = self.access_rules.withdrawable(rule);
        self
    }

    /// Sets up who can deposit tokens of the resource into any vault
    ///
    /// Allows you to pass an [`AccessRule`] that defines who can deposit tokens (via a specified amount) into a vault.
    ///
    /// By default, deposit is allowed for all users.
    ///
    /// # Examples
    ///
    /// ```rust, ignore
    /// use tari_template_lib::auth::AccessRule;
    /// use tari_template_lib::resource::builder::ResourceBuilder;
    /// ResourceBuilder::confidential()
    ///    .depositable(rule!(allow_all))
    ///     .build();
    /// ```
    pub fn depositable(mut self, rule: AccessRule) -> Self {
        self.access_rules = self.access_rules.depositable(rule);
        self
    }

    /// Sets up who can update the access rules of the resource.
    ///
    /// Allows you to pass an [`AccessRule`] that defines who can update the access rules of the resource.
    ///
    /// By default, the ability to update access rules is denied for all users. If you want to allow the owner to update
    /// the access rules, you can use `.update_access_rules(AccessRule::require_owner())`
    ///
    /// # Examples
    ///
    /// ```rust, ignore
    /// use tari_template_lib::auth::AccessRule;
    /// use tari_template_lib::resource::builder::ResourceBuilder;
    /// ResourceBuilder::confidential()
    ///     .update_access_rules(AccessRule::require_owner())
    ///     .build();
    /// ```
    pub fn update_access_rules(mut self, rule: AccessRule) -> Self {
        self.access_rules = self.access_rules.update_access_rules(rule);
        self
    }

    /// Sets up the specified `symbol` as the token symbol in the metadata of the resource
    ///
    /// # Examples
    /// ```rust, ignore
    /// use tari_template_lib::resource::builder::ResourceBuilder;
    /// ResourceBuilder::confidential()
    ///     .with_token_symbol("MY_TOKEN")
    ///     .build();
    /// ```
    pub fn with_token_symbol<S: Into<String>>(mut self, symbol: S) -> Self {
        self.token_symbol = Some(symbol.into());
        self
    }

    /// Adds a new metadata entry to the resource
    ///
    /// Allows you to add a key-value pair to the resource's metadata.
    ///
    /// # Notes
    ///
    /// `.add_metadata()` will override any existing metadata with the same key.
    ///
    /// # Examples
    /// ```rust, ignore
    /// use tari_template_lib::resource::builder::ResourceBuilder;
    /// ResourceBuilder::confidential()
    ///    .add_metadata("CharacterName", "Tari")
    ///    .add_metadata("CharacterType", "Mascot")
    ///    .add_metadata("CharacterLvl", "99")
    /// .build();
    /// ```
    pub fn add_metadata<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Replaces the resource's metadata with the given [`Metadata`] object.
    ///
    /// Note: This will overwrite any existing metadata, including entries added via `.add_metadata()`.
    ///
    /// # Examples
    /// ```rust, ignore
    /// use tari_template_lib::resource::builder::ResourceBuilder;
    /// use tari_template_lib::models::Metadata;
    /// let metadata = Metadata::from([
    ///     ("Type", "NFT"),
    ///     ("Creator", "Tari Project"),
    /// ]);
    /// let address = ResourceBuilder::confidential()
    ///     .with_metadata(metadata)
    ///     .build();
    /// ```
    pub fn with_metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Sets up the image URL of the resource
    ///
    /// Allows you to set the image URL of the resource, which can be used in user interfaces to display the token's
    /// logo or image.
    ///
    /// # Examples
    /// ```rust, ignore
    /// use tari_template_lib::resource::builder::ResourceBuilder;
    /// ResourceBuilder::confidential()
    ///     .with_image_url("https://example.com/my_token_image.png".to_string())
    ///     .build();
    /// ```
    pub fn with_image_url(self, url: String) -> Self {
        self.add_metadata(IMAGE_URL, url)
    }

    /// Sets the divisibility of the resource. i.e. the number of decimal places
    ///
    /// The default divisibility is 8, which means the smallest unit of the resource is 0.00000001 of the
    /// whole unit.
    ///
    /// # Panics
    /// method will panic if:
    /// * The divisibility is greater than 18.
    ///
    /// # Examples
    /// ```rust, ignore
    /// use tari_template_lib::resource::builder::ResourceBuilder;
    /// ResourceBuilder::confidential()
    ///     .with_divisibility(9)
    ///     .build();
    /// ```
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

    /// Build the resource, with no initial supply and returns the `ResourceAddress` of the created resource.
    ///
    /// To create a resource with an initial supply, see [ResourceBuilder::initial_supply].
    ///      
    pub fn build(self) -> ResourceAddress {
        let (address, _) = self.build_internal(None);
        address
    }

    /// Sets up how many tokens are going to be minted on resource creation based on the provided [`ConfidentialOutputStatement`]
    /// and returns a bucket containing the initial supply.
    /// 
    /// # Notes
    /// This function requires a [`ConfidentialOutputStatement`] that includes:
    /// - A valid range proof proving the output values are in range [minimum_value_promise, 2^64)
    /// - (Optional) Confidential outputs (as `UnspentOutput`s) for the recipient and change
    /// - (Optional) Revealed output and change amounts
    ///
    /// Use [`ConfidentialOutputStatement::mint_revealed()`] to mint **revealed funds only** (no commitments). 
    /// 
    pub fn initial_supply(self, initial_supply_proof: ConfidentialOutputStatement) -> Bucket {
        let mint_arg = MintArg::Confidential {
            statement: Box::new(initial_supply_proof),
        };

        let (_, bucket) = self.build_internal(Some(mint_arg));
        bucket.expect("[initial_supply] Bucket not returned from system")
    }

    fn build_internal(mut self, mint_arg: Option<MintArg>) -> (ResourceAddress, Option<Bucket>) {
        if let Some(symbol) = self.token_symbol {
            self.metadata.insert(TOKEN_SYMBOL, symbol);
        }
        ResourceManager::create(
            ResourceType::Confidential,
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
