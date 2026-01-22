//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::resource::Resource;
use tari_template_lib::types::ResourceAddress;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResourceModel {
    pub address: ResourceAddress,
    pub resource: Resource,
}

impl From<ResourceModel> for Resource {
    fn from(model: ResourceModel) -> Self {
        model.resource
    }
}
