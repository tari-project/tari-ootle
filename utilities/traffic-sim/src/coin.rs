//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib_types::{ComponentAddress, ResourceAddress, TemplateAddress};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Coin {
    pub template_address: TemplateAddress,
    pub component_address: ComponentAddress,
    pub resource_address: ResourceAddress,
    pub admin_badge: ResourceAddress,
}
