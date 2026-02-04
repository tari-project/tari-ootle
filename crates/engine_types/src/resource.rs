//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::borrow::Cow;

use ootle_byte_type::FromByteType;
use serde::{Deserialize, Serialize};
use tari_crypto::{ristretto::RistrettoPublicKey, tari_utilities::ByteArrayError};
use tari_template_lib::{
    auth::Ownership,
    resource::TOKEN_SYMBOL,
    types::{
        Amount,
        AuthHook,
        Metadata,
        OwnerRule,
        ResourceType,
        access_rules::ResourceAccessRules,
        crypto::RistrettoPublicKeyBytes,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Resource {
    resource_type: ResourceType,
    owner_rule: OwnerRule,
    owner_key: Option<RistrettoPublicKeyBytes>,
    access_rules: ResourceAccessRules,
    metadata: Metadata,
    /// The total supply of the resource. None means total_supply tracking is disabled.
    total_supply: Option<Amount>,
    view_key: Option<RistrettoPublicKeyBytes>,
    auth_hook: Option<AuthHook>,
    divisibility: u8,
}

impl Resource {
    pub const fn new(
        resource_type: ResourceType,
        owner_key: Option<RistrettoPublicKeyBytes>,
        owner_rule: OwnerRule,
        access_rules: ResourceAccessRules,
        metadata: Metadata,
        view_key: Option<RistrettoPublicKeyBytes>,
        auth_hook: Option<AuthHook>,
        mut divisibility: u8,
        is_total_supply_tracking_enabled: bool,
    ) -> Self {
        // TODO: improve API to make it impossible to set incorrect divisibility
        if resource_type.is_non_fungible() {
            divisibility = 0;
        }

        Self {
            resource_type,
            owner_rule,
            owner_key,
            access_rules,
            metadata,
            total_supply: if is_total_supply_tracking_enabled {
                Some(Amount::zero())
            } else {
                None
            },
            divisibility,
            view_key,
            auth_hook,
        }
    }

    pub fn load(
        resource_type: ResourceType,
        owner_key: Option<RistrettoPublicKeyBytes>,
        owner_rule: OwnerRule,
        access_rules: ResourceAccessRules,
        metadata: Metadata,
        view_key: Option<RistrettoPublicKeyBytes>,
        auth_hook: Option<AuthHook>,
        divisibility: u8,
        total_supply: Option<Amount>,
    ) -> Self {
        Self {
            resource_type,
            owner_rule,
            owner_key,
            access_rules,
            metadata,
            total_supply,
            view_key,
            auth_hook,
            divisibility,
        }
    }

    pub fn resource_type(&self) -> ResourceType {
        self.resource_type
    }

    pub fn owner_rule(&self) -> &OwnerRule {
        &self.owner_rule
    }

    pub fn owner_key(&self) -> Option<&RistrettoPublicKeyBytes> {
        self.owner_key.as_ref()
    }

    pub fn as_ownership(&self) -> Ownership<'_> {
        Ownership {
            owner_key: self.owner_key.as_ref(),
            owner_rule: Cow::Borrowed(&self.owner_rule),
        }
    }

    pub fn view_key(&self) -> Option<&RistrettoPublicKeyBytes> {
        self.view_key.as_ref()
    }

    /// Converts the view key to a `RistrettoPublicKey`, returning `None` if the view key is not set
    /// or returning an error if the view key is not a canonical compressed representation of a Ristretto public key.
    pub fn to_view_key_public_key(&self) -> Result<Option<RistrettoPublicKey>, ByteArrayError> {
        match self.view_key.as_ref() {
            Some(view_key) => view_key.try_from_byte_type().map(Some),
            None => Ok(None),
        }
    }

    pub fn auth_hook(&self) -> Option<&AuthHook> {
        self.auth_hook.as_ref()
    }

    pub fn access_rules(&self) -> &ResourceAccessRules {
        &self.access_rules
    }

    pub fn set_access_rules(&mut self, access_rules: ResourceAccessRules) {
        self.access_rules = access_rules;
    }

    /// Returns `true` if the resource has enabled supply tracking, otherwise `false`
    pub fn is_supply_tracking_enabled(&self) -> bool {
        self.total_supply.is_some()
    }

    /// Increases the total supply. This is a no-op if total supply tracking is disabled.
    /// Returns `true` if the total supply was successfully increased or supply tracking is disabled, or `false` if it
    /// would overflow.
    ///
    /// ## Panics
    /// Panics if the amount is not positive
    pub fn increase_total_supply(&mut self, amount: Amount) -> bool {
        assert!(
            amount.is_non_negative(),
            "Invariant violation in increase_total_supply: amount must be non-negative but was {}",
            amount
        );
        let Some(supply_mut) = self.total_supply.as_mut() else {
            // Total supply tracking is disabled, this call succeeded
            return true;
        };
        let next_supply = supply_mut.checked_add(amount);
        match next_supply {
            Some(new_supply) => {
                *supply_mut = new_supply;
                true
            },
            None => false,
        }
    }

    /// Decreases the total supply. This is a no-op if total supply tracking is disabled.
    ///
    /// ## Panics
    /// Panics if the amount is not positive or if the amount is greater than the total supply.
    pub fn decrease_total_supply(&mut self, amount: Amount) {
        assert!(
            amount.is_non_negative(),
            "Invariant violation in decrease_total_supply: amount must be positive"
        );
        if let Some(supply_mut) = self.total_supply.as_mut() {
            *supply_mut = supply_mut.checked_sub_positive(amount).expect(
                "Invariant violation in decrease_total_supply: decrease total supply by more than total supply",
            );
        }
    }

    /// Returns the total supply of the resource, or `None` if total supply tracking is disabled.
    pub fn total_supply(&self) -> Option<Amount> {
        self.total_supply
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    pub fn token_symbol(&self) -> Option<&str> {
        self.metadata.get(TOKEN_SYMBOL)
    }

    pub fn divisibility(&self) -> u8 {
        self.divisibility
    }
}
