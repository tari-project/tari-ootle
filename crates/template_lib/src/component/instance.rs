//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::Encode;
use tari_template_abi::rust::marker::PhantomData;
use tari_template_lib_types::{
    ComponentAddress,
    OwnerRule,
    access_rules::{ComponentAccessRules, RulesUpdater, UpdateRule},
};

use crate::{
    caller_context::CallerContext,
    engine,
    models::ComponentAddressAllocation,
    types::crypto::RistrettoPublicKeyBytes,
};

/// Utility for building components inside templates
pub struct ComponentBuilder<T> {
    component: T,
    owner_rule: OwnerRule,
    access_rules: ComponentAccessRules,
    address_allocation: Option<ComponentAddressAllocation>,
    rules_updater: RulesUpdater,
}

impl<T> ComponentBuilder<T> {
    /// Returns a new component builder for the specified data
    fn new(component: T) -> Self {
        Self {
            component,
            owner_rule: OwnerRule::default(),
            access_rules: ComponentAccessRules::new(),
            address_allocation: None,
            rules_updater: RulesUpdater::Locked,
        }
    }

    /// Use an allocated address for the component.
    pub fn with_address_allocation(self, allocation: ComponentAddressAllocation) -> Self {
        self.with_address_allocation_opt(Some(allocation))
    }

    /// Use an allocated address for the component, if `Some`.
    pub fn with_address_allocation_opt(mut self, allocation: Option<ComponentAddressAllocation>) -> Self {
        if let Some(allocation) = allocation {
            assert!(self.address_allocation.is_none(), "Address allocation already set");
            self.address_allocation = Some(allocation);
        }
        self
    }

    /// Equivalent to :
    /// ```rust,ignore
    /// let allocation = CallerContext::allocate_component_address(Some(pk));
    /// Component::new(..)
    /// .with_address_allocation(allocation)
    /// ```
    /// Calling this followed by `with_address_allocation` (or vice versa) will cause execution to fail
    pub fn with_public_key_address(self, public_key: RistrettoPublicKeyBytes) -> Self {
        self.with_address_allocation(CallerContext::allocate_component_address(Some(public_key)))
    }

    /// Sets up who will be the owner of the component. The owner may call every method. Whether the owner may also
    /// replace the access rules or the owner rule is set by [`Self::with_rules_updater`].
    pub fn with_owner_rule(mut self, owner_rule: OwnerRule) -> Self {
        self.owner_rule = owner_rule;
        self
    }

    /// Sets up who can access each of the component's methods
    pub fn with_access_rules(mut self, access_rules: ComponentAccessRules) -> Self {
        self.access_rules = access_rules;
        self
    }

    /// Sets up who may replace the component's access rules and owner rule, by transaction instruction or through
    /// `ComponentManager::set_access_rules`/`set_owner_rule`. The setting is fixed at creation. Defaults to
    /// `RulesUpdater::Locked`, under which the rules never change. `RulesUpdater::Owner` or `RulesUpdater::AccessRule`
    /// let the rules change by instruction or from the component's own code; `RulesUpdater::TemplateManaged` is for a
    /// template that must run its own logic around every change.
    pub fn with_rules_updater(mut self, rules_updater: RulesUpdater) -> Self {
        self.rules_updater = rules_updater;
        self
    }
}

impl<T: Encode<()>> ComponentBuilder<T> {
    /// Creates the new component and returns it
    pub fn create(self) -> Component<T> {
        let address = engine().create_component(
            self.component,
            self.owner_rule,
            self.access_rules,
            self.address_allocation,
            UpdateRule::Locked,
            self.rules_updater,
        );
        Component::from_address(address)
    }
}

/// A newly created component, typically used as a return value from template constructor functions
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
pub struct Component<T> {
    address: ComponentAddress,
    #[cfg_attr(feature = "serde", serde(skip))]
    _component: PhantomData<T>,
}

// Encode/Decode/CborLen are hand-rolled so the wire format matches ComponentAddress
// (transparent over the only non-skipped field).
impl<C, T> minicbor::Encode<C> for Component<T> {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        self.address.encode(e, ctx)
    }
}

impl<'b, C, T> minicbor::Decode<'b, C> for Component<T> {
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        Ok(Self {
            address: ComponentAddress::decode(d, ctx)?,
            _component: PhantomData,
        })
    }
}

impl<C, T> minicbor::CborLen<C> for Component<T> {
    fn cbor_len(&self, ctx: &mut C) -> usize {
        self.address.cbor_len(ctx)
    }
}

impl<T: Encode<()>> Component<T> {
    /// Returns a new component builder for the specified data
    #[allow(clippy::new_ret_no_self)]
    pub fn new(component: T) -> ComponentBuilder<T> {
        ComponentBuilder::new(component)
    }

    /// Creates the new component and returns it
    pub fn create(component: T) -> Component<T> {
        Self::new(component).create()
    }

    /// Creates a new component with the specified address
    fn from_address(address: ComponentAddress) -> Self {
        Self {
            address,
            _component: PhantomData,
        }
    }

    /// Returns the address of the component
    pub fn address(&self) -> &ComponentAddress {
        &self.address
    }
}

#[cfg(test)]
mod tests {
    use tari_bor::{decode, encode};
    use tari_template_lib_types::ObjectKey;

    use super::*;

    #[test]
    fn it_serializes_as_a_component_address() {
        decode::<ComponentAddress>(
            &encode(&Component::<u32>::from_address(ComponentAddress::new(
                ObjectKey::default(),
            )))
            .unwrap(),
        )
        .unwrap();
    }
}
