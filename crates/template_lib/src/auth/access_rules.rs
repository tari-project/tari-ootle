//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_abi::rust::collections::BTreeMap;
use tari_template_lib_types::{ComponentAddress, NonFungibleAddress, ResourceAddress, TemplateAddress};

/// Represents the types of possible access control rules over a component method or resource
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum AccessRule {
    /// AccessRule always passes
    AllowAll,
    /// AccessRule always fails
    DenyAll,
    /// AccessRule that requires a specific condition to be met
    Restricted(RestrictedAccessRule),
}

impl AccessRule {
    pub fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::AllowAll, Self::AllowAll) => Self::AllowAll,
            (Self::DenyAll, _) | (_, Self::DenyAll) => Self::DenyAll,
            (Self::Restricted(rule1), Self::Restricted(rule2)) => Self::Restricted(rule1.and(rule2)),
            (Self::Restricted(rule), Self::AllowAll) | (Self::AllowAll, Self::Restricted(rule)) => {
                Self::Restricted(rule)
            },
        }
    }

    pub fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::AllowAll, _) | (_, Self::AllowAll) => Self::AllowAll,
            (Self::DenyAll, Self::DenyAll) => Self::DenyAll,
            (Self::Restricted(rule1), Self::Restricted(rule2)) => Self::Restricted(rule1.or(rule2)),
            (Self::Restricted(rule), Self::DenyAll) | (Self::DenyAll, Self::Restricted(rule)) => Self::Restricted(rule),
        }
    }
}

/// An enum that represents the possible ways to restrict access to components or resources
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum RestrictedAccessRule {
    /// Requires a specific condition to be met
    Require(RequireRule),
    /// Requires any of the specified conditions to be met (logical OR)
    AnyOf(Box<[RestrictedAccessRule]>),
    /// Requires all of the specified conditions to be met (logical AND)
    AllOf(Box<[RestrictedAccessRule]>),
}

impl RestrictedAccessRule {
    pub fn and(self, other: Self) -> Self {
        Self::AllOf(Box::new([self, other]))
    }

    pub fn or(self, other: Self) -> Self {
        Self::AnyOf(Box::new([self, other]))
    }
}

/// Specifies a requirement for a [RequireRule].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum RuleRequirement {
    /// Requires a proof of a specific resource
    Resource(ResourceAddress),
    /// Requires a proof of a specific non-fungible token
    NonFungibleAddress(NonFungibleAddress),
    /// Requires execution within a specific component
    ScopedToComponent(ComponentAddress),
    /// Requires execution within a specific template
    ScopedToTemplate(TemplateAddress),
}

impl From<ResourceAddress> for RuleRequirement {
    fn from(address: ResourceAddress) -> Self {
        Self::Resource(address)
    }
}

impl From<NonFungibleAddress> for RuleRequirement {
    fn from(address: NonFungibleAddress) -> Self {
        Self::NonFungibleAddress(address)
    }
}

impl From<ComponentAddress> for RuleRequirement {
    fn from(address: ComponentAddress) -> Self {
        Self::ScopedToComponent(address)
    }
}

impl From<TemplateAddress> for RuleRequirement {
    fn from(address: TemplateAddress) -> Self {
        Self::ScopedToTemplate(address)
    }
}

/// A rule requiring specific condition(s) to be met
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum RequireRule {
    /// Requires a specific condition to be met
    Require(RuleRequirement),
    /// Requires any of the specified conditions to be met (logical OR)
    AnyOf(Box<[RuleRequirement]>),
    /// Requires all of the specified conditions to be met (logical AND)
    AllOf(Box<[RuleRequirement]>),
    /// Requires N of the specified conditions to be met
    MOfN(u16, Box<[RuleRequirement]>),
}

/// Information needed to specify access rules to methods of a component
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct ComponentAccessRules {
    #[cfg_attr(feature = "ts", ts(type = "Record<string, AccessRule>"))]
    method_access: BTreeMap<String, AccessRule>,
    default: AccessRule,
}

impl ComponentAccessRules {
    /// Builds a new set of access rules for a component.
    /// By default, all methods of the component are inaccessible and must be explicitly allowed
    pub fn new() -> Self {
        Self {
            method_access: BTreeMap::new(),
            default: AccessRule::DenyAll,
        }
    }

    /// Builds a new set of access rules for a component, using by default that anyone can call any method on the
    /// component
    pub fn allow_all() -> Self {
        Self {
            method_access: BTreeMap::new(),
            default: AccessRule::AllowAll,
        }
    }

    /// Add a new access rule for a particular method in the component
    pub fn add_method_rule<S: Into<String>>(mut self, name: S, rule: AccessRule) -> Self {
        self.method_access.insert(name.into(), rule);
        self
    }

    /// Returns the number of custom access rules
    pub fn num_access_rules(&self) -> usize {
        self.method_access.len()
    }

    /// Set up the default access rule for all methods that do not have a specific rule
    pub fn default(mut self, rule: AccessRule) -> Self {
        self.default = rule;
        self
    }

    /// Return the access rule of a particular method in the component
    pub fn get_method_access_rule(&self, name: &str) -> &AccessRule {
        self.method_access.get(name).unwrap_or(&self.default)
    }

    /// Return an iterator over the access rules of all methods
    pub fn method_access_rules_iter(&self) -> impl Iterator<Item = (&String, &AccessRule)> {
        self.method_access.iter()
    }
}

impl Default for ComponentAccessRules {
    fn default() -> Self {
        Self::new()
    }
}

/// An enum that represents all the possible actions that can be performed on a resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResourceAuthAction {
    Mint,
    Burn,
    Recall,
    Withdraw,
    Deposit,
    UpdateNonFungibleData,
    UpdateAccessRules,
    Freeze,
}

impl ResourceAuthAction {
    pub fn is_recall(&self) -> bool {
        matches!(self, Self::Recall)
    }
}

/// Information needed to specify access rules to a resource
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ResourceAccessRules {
    mintable: AccessRule,
    burnable: AccessRule,
    recallable: AccessRule,
    withdrawable: AccessRule,
    depositable: AccessRule,
    update_non_fungible_data: AccessRule,
    update_access_rules: AccessRule,
    freeze: AccessRule,
}

impl ResourceAccessRules {
    /// Builds a new set of access rules for a resource.
    ///
    /// By default:
    /// * Updating the access rules is disabled for all users (i.e. only the OwnerRule applies)
    /// * Minting, burning, recalling and freezing are disabled for all users
    /// * Withdrawals, deposits and non-fungible data updates are allowed for all users
    pub const fn new() -> Self {
        Self {
            // User should explicitly enable minting, burning etc
            mintable: AccessRule::DenyAll,
            burnable: AccessRule::DenyAll,
            recallable: AccessRule::DenyAll,
            update_access_rules: AccessRule::DenyAll,
            freeze: AccessRule::DenyAll,
            // But explicitly disable withdrawing, updating and/or depositing
            withdrawable: AccessRule::AllowAll,
            depositable: AccessRule::AllowAll,
            update_non_fungible_data: AccessRule::AllowAll,
        }
    }

    /// Update the access rules so no one can perform any action on the resource after its creation
    pub fn deny_all() -> Self {
        Self {
            mintable: AccessRule::DenyAll,
            burnable: AccessRule::DenyAll,
            recallable: AccessRule::DenyAll,
            update_access_rules: AccessRule::DenyAll,
            withdrawable: AccessRule::DenyAll,
            depositable: AccessRule::DenyAll,
            update_non_fungible_data: AccessRule::DenyAll,
            freeze: AccessRule::DenyAll,
        }
    }

    /// Sets up who can mint new tokens of the resource
    pub fn mintable(mut self, rule: AccessRule) -> Self {
        self.mintable = rule;
        self
    }

    /// Sets up who can burn (destroy) tokens of the resource
    pub fn burnable(mut self, rule: AccessRule) -> Self {
        self.burnable = rule;
        self
    }

    /// Sets up who can recall tokens of the resource.
    /// A recall is the forceful withdrawal of tokens from any external vault
    pub fn recallable(mut self, rule: AccessRule) -> Self {
        self.recallable = rule;
        self
    }

    /// Sets up who can freeze a vault containing this resource, preventing withdrawals.
    pub fn freezable(mut self, rule: AccessRule) -> Self {
        self.freeze = rule;
        self
    }

    /// Sets up who can withdraw tokens of the resource from any vault
    pub fn withdrawable(mut self, rule: AccessRule) -> Self {
        self.withdrawable = rule;
        self
    }

    /// Sets up who can deposit tokens of the resource into any vault
    pub fn depositable(mut self, rule: AccessRule) -> Self {
        self.depositable = rule;
        self
    }

    /// Sets up who can update the access rules of the resource
    pub fn update_access_rules(mut self, rule: AccessRule) -> Self {
        self.update_access_rules = rule;
        self
    }

    /// Sets up who can update the mutable data of the tokens in the resource
    pub fn update_non_fungible_data(mut self, rule: AccessRule) -> Self {
        self.update_non_fungible_data = rule;
        self
    }

    /// Returns a reference to the access rule for the specified action
    pub fn get_access_rule(&self, action: &ResourceAuthAction) -> &AccessRule {
        match action {
            ResourceAuthAction::Mint => &self.mintable,
            ResourceAuthAction::Burn => &self.burnable,
            ResourceAuthAction::Recall => &self.recallable,
            ResourceAuthAction::Withdraw => &self.withdrawable,
            ResourceAuthAction::Deposit => &self.depositable,
            ResourceAuthAction::UpdateNonFungibleData => &self.update_non_fungible_data,
            ResourceAuthAction::UpdateAccessRules => &self.update_access_rules,
            ResourceAuthAction::Freeze => &self.freeze,
        }
    }
}

impl Default for ResourceAccessRules {
    fn default() -> Self {
        Self::new()
    }
}

/// A macro to build access rules for components and resources.
///
/// It allows for defining rules such as `allow_all`, `deny_all`, and more complex rules using `any_of`, `all_of` and
/// `n_of` constructs.
///
/// # Examples:
///
/// ```rust
/// use tari_template_lib::rule;
/// // Allow all access
/// let allow_all_rule = rule!(allow_all);
/// // Deny all access
/// let deny_all_rule = rule!(deny_all);
/// // Restricted access to a specific resource
/// let resource_address = tari_template_lib::types::ResourceAddress::new(
///     tari_template_lib::types::ObjectKey::default(),
/// );
/// let resource_rule = rule!(resource(resource_address));
/// // Restricted access to a component
/// let component_address = tari_template_lib::types::ComponentAddress::new(
///     tari_template_lib::types::ObjectKey::default(),
/// );
/// let component_rule = rule!(component(component_address));
/// // Restricted access to a template
/// let template_address = tari_template_lib::types::TemplateAddress::default();
/// let template_rule = rule!(template(template_address));
/// // Restricted access to a non-fungible token
/// let non_fungible_address = tari_template_lib::types::NonFungibleAddress::from_public_key(
///     tari_template_lib::types::crypto::RistrettoPublicKeyBytes::default(),
/// );
/// let non_fungible_rule = rule!(non_fungible(non_fungible_address));
/// // Complex rules using `any_of`, `all_of` and `n_of`
/// let complex_rule = rule!(any_of(
///     component(component_address),
///     resource(resource_address)
/// ));
/// # let pk1 = tari_template_lib::types::crypto::RistrettoPublicKeyBytes::default();
/// # let pk2 = tari_template_lib::types::crypto::RistrettoPublicKeyBytes::default();
/// let n_of_rule = rule!(m_of_n(2, public_key(pk1), public_key(pk2)));
/// ```
#[macro_export]
macro_rules! rule {
    (allow_all) => {
        $crate::auth::AccessRule::AllowAll
    };
    (deny_all) => {
        $crate::auth::AccessRule::DenyAll
    };
    ($($tail:tt)*) => {
        $crate::auth::AccessRule::Restricted($crate::__restricted_access_rule!($($tail)*))
    };
}

#[macro_export]
macro_rules! __restricted_access_rule {
    (any_of($($tail:tt)*)) => {
        $crate::auth::RestrictedAccessRule::AnyOf($crate::__build_vec!(@ {__restricted_access_rule} $($tail)*).into_boxed_slice())
    };
    (all_of($($tail:tt)*)) => {
        $crate::auth::RestrictedAccessRule::AllOf($crate::__build_vec!(@ {__restricted_access_rule} $($tail)*).into_boxed_slice())
    };
    ($a:ident($($tail:tt)*)) => {
        $crate::auth::RestrictedAccessRule::Require($crate::__require_rule!($a($($tail)*)))
    };
}

#[macro_export]
macro_rules! __require_rule {
    (any_of($($tail:tt)*)) => {
        $crate::auth::RequireRule::AnyOf($crate::__build_vec!(@ {__rule_requirement} $($tail)*).into_boxed_slice())
    };
    (all_of($($tail:tt)*)) => {
        $crate::auth::RequireRule::AllOf($crate::__build_vec!(@ {__rule_requirement} $($tail)*).into_boxed_slice())
    };
    (m_of_n($n:literal, $($tail:tt)*)) => {
        $crate::auth::RequireRule::MOfN($n, $crate::__build_vec!(@ {__rule_requirement} $($tail)*).into_boxed_slice())
    };
    ($a:ident($b:expr)) => {
        $crate::auth::RequireRule::Require($crate::__rule_requirement!($a($b)))
    };
}

#[macro_export]
macro_rules! __rule_requirement {
    (resource($x: expr)) => {
        $crate::auth::RuleRequirement::Resource($x)
    };
    (non_fungible($x: expr)) => {
        $crate::auth::RuleRequirement::NonFungibleAddress($x)
    };
    (public_key($x: expr)) => {
        $crate::auth::RuleRequirement::NonFungibleAddress($crate::types::NonFungibleAddress::from_public_key($x))
    };
    (component($x: expr)) => {
        $crate::auth::RuleRequirement::ScopedToComponent($x)
    };
    (template($x: expr)) => {
        $crate::auth::RuleRequirement::ScopedToTemplate($x)
    };
}

#[macro_export]
macro_rules! __build_vec {
    () => (Vec::new());

    (@ {$item_fn:ident} $a:ident($b:expr), $($tail:tt)*) => {{
        let mut items = Vec::with_capacity(1 + $crate::__expr_counter!($($tail)*));
        $crate::__build_vec_inner!(@ { items, $item_fn } $a($b), $($tail)*);
        items
    }};

    (@ {$item_fn:ident} $a:ident($b:expr) $(,)?) => {{
        let mut items = Vec::new();
        $crate::__build_vec_inner!(@ { items, $item_fn } $a($b),);
        items
    }};
}

#[macro_export]
macro_rules! __build_vec_inner {
    (@ { $this:ident, $item_fn:ident } $a:ident($e:expr), $($tail:tt)*) => {
        $crate::args::__push(&mut $this, $crate::$item_fn!($a($e)));
        $crate::__build_vec_inner!(@ {$this, $item_fn } $($tail)*);
    };
    (@ { $this:ident, $item_fn:ident } $a:ident($e:expr) $(,)*) => {
        $crate::args::__push(&mut $this, $crate::$item_fn!($a($e)));
    };
}

#[cfg(test)]
mod tests {
    use tari_template_lib_types::ObjectKey;

    use super::*;
    use crate::types::crypto::RistrettoPublicKeyBytes;

    #[test]
    fn it_builds_correct_access_rules() {
        // allow all
        let rule = rule!(allow_all);
        assert_eq!(rule, AccessRule::AllowAll);

        // deny all
        let rule = rule!(deny_all);
        assert_eq!(rule, AccessRule::DenyAll);

        // restricted to resource address
        let resource_address = ResourceAddress::new(ObjectKey::default());
        let rule = rule!(resource(resource_address));
        assert_eq!(
            rule,
            access_rule_from_requirement(RuleRequirement::Resource(resource_address))
        );

        // restricted to component
        let component_address = ComponentAddress::new(ObjectKey::default());
        let rule = rule!(component(component_address));
        assert_eq!(
            rule,
            access_rule_from_requirement(RuleRequirement::ScopedToComponent(component_address))
        );

        // restricted to template
        let template_address = TemplateAddress::default();
        let rule = rule!(template(template_address));
        assert_eq!(
            rule,
            access_rule_from_requirement(RuleRequirement::ScopedToTemplate(template_address))
        );

        // restricted to non fungible
        let non_fungible_address = NonFungibleAddress::from_public_key(RistrettoPublicKeyBytes::default());
        let rule = rule!(non_fungible(non_fungible_address.clone()));
        assert_eq!(
            rule,
            access_rule_from_requirement(RuleRequirement::NonFungibleAddress(non_fungible_address))
        );

        // composition of rules
        let rule = rule!(any_of(component(component_address), resource(resource_address)));
        assert_eq!(
            rule,
            AccessRule::Restricted(RestrictedAccessRule::AnyOf(Box::new([
                RestrictedAccessRule::Require(RequireRule::Require(RuleRequirement::ScopedToComponent(
                    component_address
                ))),
                RestrictedAccessRule::Require(RequireRule::Require(RuleRequirement::Resource(resource_address))),
            ])))
        );

        let rule = rule!(all_of(component(component_address), resource(resource_address)));
        assert_eq!(
            rule,
            AccessRule::Restricted(RestrictedAccessRule::AllOf(Box::new([
                RestrictedAccessRule::Require(RequireRule::Require(RuleRequirement::ScopedToComponent(
                    component_address
                ))),
                RestrictedAccessRule::Require(RequireRule::Require(RuleRequirement::Resource(resource_address))),
            ])))
        );

        let rule = rule!(m_of_n(1, component(component_address), resource(resource_address)));
        assert_eq!(
            rule,
            AccessRule::Restricted(RestrictedAccessRule::Require(RequireRule::MOfN(
                1,
                Box::new([
                    RuleRequirement::ScopedToComponent(component_address),
                    RuleRequirement::Resource(resource_address),
                ])
            )))
        );
    }

    fn access_rule_from_requirement(requirement: RuleRequirement) -> AccessRule {
        AccessRule::Restricted(RestrictedAccessRule::Require(RequireRule::Require(requirement)))
    }
}
