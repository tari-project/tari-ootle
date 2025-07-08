//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::models::ComponentAddress;

pub enum NamedComponentCall {
    Address(ComponentAddress),
    Workspace(CallFromWorkspace),
}

pub struct CallFromWorkspace(pub String);

impl CallFromWorkspace {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn name(&self) -> &str {
        &self.0
    }
}

impl From<ComponentAddress> for NamedComponentCall {
    fn from(address: ComponentAddress) -> Self {
        Self::Address(address)
    }
}

impl From<CallFromWorkspace> for NamedComponentCall {
    fn from(workspace: CallFromWorkspace) -> Self {
        Self::Workspace(workspace)
    }
}

impl From<&str> for NamedComponentCall {
    fn from(name: &str) -> Self {
        Self::Workspace(CallFromWorkspace::new(name))
    }
}
