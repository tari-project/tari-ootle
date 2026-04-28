//   Copyright 2023. The Tari Project
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

use serde::{Deserialize, Serialize};
use tari_template_lib::types::{
    ComponentAddress,
    EntityId,
    ObjectKey,
    SubstateOwnerRule,
    TemplateAddress,
    access_rules::ComponentAccessRules,
    crypto::RistrettoPublicKeyBytes,
};

use crate::{
    hashing::{EngineHashDomainLabel, hasher32},
    indexed_value::{IndexedValueError, IndexedWellKnownTypes},
    ownership::Ownership,
    substate::SubstateId,
};

/// Derives a component address.
///
/// This can be used to derive the component address from a public key if the component sets public_key_address in the
/// component builder.
pub fn derive_component_address_from_public_key(
    template_address: &TemplateAddress,
    public_key: &RistrettoPublicKeyBytes,
) -> ComponentAddress {
    let address = hasher32(EngineHashDomainLabel::ComponentAddress)
        .chain(template_address)
        .chain(public_key)
        .result();
    let key = ObjectKey::from_array(address.leading_bytes());
    ComponentAddress::new(key)
}

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Component {
    pub header: ComponentHeader,
    pub body: ComponentBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ComponentHeader {
    pub template_address: TemplateAddress,
    pub owner_rule: SubstateOwnerRule,
    pub access_rules: ComponentAccessRules,
    pub entity_id: EntityId,
}

impl Component {
    pub fn into_component(self) -> ComponentBody {
        self.body
    }

    pub fn state(&self) -> &tari_bor::Value {
        &self.body.state
    }

    pub fn into_state(self) -> tari_bor::Value {
        self.body.state
    }

    pub fn as_ownership(&self) -> Ownership<'_> {
        Ownership {
            owner_rule: Cow::Borrowed(&self.header.owner_rule),
        }
    }

    pub fn header(&self) -> &ComponentHeader {
        &self.header
    }

    pub fn body(&self) -> &ComponentBody {
        &self.body
    }

    pub fn owner_rule(&self) -> &SubstateOwnerRule {
        &self.header.owner_rule
    }

    pub fn entity_id(&self) -> &EntityId {
        &self.header.entity_id
    }

    pub fn owner_public_key(&self) -> Option<&RistrettoPublicKeyBytes> {
        self.header.owner_rule.owned_by_public_key()
    }

    pub fn access_rules(&self) -> &ComponentAccessRules {
        &self.header.access_rules
    }

    pub fn template_address(&self) -> &TemplateAddress {
        &self.header.template_address
    }

    pub fn set_access_rules(&mut self, access_rules: ComponentAccessRules) -> &mut Self {
        self.header.access_rules = access_rules;
        self
    }

    pub fn set_template_address(&mut self, template_address: TemplateAddress) -> &mut Self {
        self.header.template_address = template_address;
        self
    }

    pub fn contains_substate(&self, address: &SubstateId) -> Result<bool, IndexedValueError> {
        let found = IndexedWellKnownTypes::value_contains_substate(self.state(), address)?;
        Ok(found)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ComponentBody {
    #[serde(with = "ootle_serde::cbor_value")]
    #[cfg_attr(feature = "ts", ts(type = "any"))]
    #[borsh(serialize_with = "crate::borsh::serialize_cbor_value")]
    pub state: tari_bor::Value,
}

impl ComponentBody {
    pub const fn empty() -> Self {
        Self {
            state: tari_bor::Value::Null,
        }
    }

    pub const fn from_cbor_value(state: tari_bor::Value) -> Self {
        Self { state }
    }

    pub fn set(&mut self, state: tari_bor::Value) -> &mut Self {
        self.state = state;
        self
    }

    pub fn to_indexed_well_known_types(&self) -> Result<IndexedWellKnownTypes, IndexedValueError> {
        IndexedWellKnownTypes::from_value(&self.state)
    }
}
