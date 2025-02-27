//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::{
    substate::{InvalidSubstateIdVariant, SubstateId},
    TemplateAddress,
};
use tari_template_lib::models::{ComponentAddress, ResourceAddress};

#[derive(Debug, Clone)]
pub struct AllocatedAddress {
    address: SubstateId,
    template_address: Option<TemplateAddress>,
}

impl AllocatedAddress {
    pub fn new(address: SubstateId, template_address: Option<TemplateAddress>) -> Self {
        Self {
            address,
            template_address,
        }
    }

    pub fn address(&self) -> &SubstateId {
        &self.address
    }

    pub fn template_address(&self) -> Option<&TemplateAddress> {
        self.template_address.as_ref()
    }
}

impl TryFrom<AllocatedAddress> for ComponentAddress {
    type Error = InvalidSubstateIdVariant;

    fn try_from(value: AllocatedAddress) -> Result<Self, Self::Error> {
        value.address.try_into()
    }
}

impl TryFrom<AllocatedAddress> for ResourceAddress {
    type Error = InvalidSubstateIdVariant;

    fn try_from(value: AllocatedAddress) -> Result<Self, Self::Error> {
        value.address.try_into()
    }
}
