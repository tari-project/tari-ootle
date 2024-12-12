// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::TemplateAddress;

/// A request for a template to be downloaded from another shard group that owns it.
pub struct TemplateSyncRequest {
    address: TemplateAddress,
}

impl TemplateSyncRequest {
    pub fn new(address: TemplateAddress) -> Self {
        Self { address }
    }
}