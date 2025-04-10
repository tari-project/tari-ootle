//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::substate::SubstateId;
use tari_template_lib::types::TemplateAddress;

#[derive(Debug, Clone)]
pub struct AllocatedAddress {
    substate_id: SubstateId,
    template_address: Option<TemplateAddress>,
}

impl AllocatedAddress {
    pub fn new(substate_id: SubstateId, template_address: Option<TemplateAddress>) -> Self {
        Self {
            substate_id,
            template_address,
        }
    }

    pub fn substate_id(&self) -> &SubstateId {
        &self.substate_id
    }

    pub fn template_address(&self) -> Option<&TemplateAddress> {
        self.template_address.as_ref()
    }
}
