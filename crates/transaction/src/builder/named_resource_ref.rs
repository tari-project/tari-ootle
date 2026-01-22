//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::types::ResourceAddress;

use crate::builder::CallFromWorkspace;

pub enum NamedResourceRef {
    Address(ResourceAddress),
    Workspace(CallFromWorkspace),
}

impl From<ResourceAddress> for NamedResourceRef {
    fn from(address: ResourceAddress) -> Self {
        Self::Address(address)
    }
}

impl From<CallFromWorkspace> for NamedResourceRef {
    fn from(workspace: CallFromWorkspace) -> Self {
        Self::Workspace(workspace)
    }
}

impl From<&str> for NamedResourceRef {
    fn from(name: &str) -> Self {
        Self::Workspace(CallFromWorkspace::new(name))
    }
}
