// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::models::TemplateAddress;

/// A request for a template to be downloaded from another shard group that owns it.
#[derive(Debug, Clone)]
pub struct TemplateSyncRequest {
    /// Address of the template to sync.
    address: TemplateAddress,
}

impl TemplateSyncRequest {
    pub fn new(address: TemplateAddress) -> Self {
        Self { address }
    }

    pub fn address(&self) -> TemplateAddress {
        self.address
    }
}
