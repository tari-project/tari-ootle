//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use tari_bor::{Deserialize, Serialize};
use tari_template_lib::types::ComponentAddress;

use crate::args::WorkspaceId;

/// A reference to a component, either by its address or by a workspace ID.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ComponentReference {
    Address(ComponentAddress),
    Workspace(WorkspaceId),
}

impl ComponentReference {
    pub fn address(&self) -> Option<&ComponentAddress> {
        match self {
            Self::Address(address) => Some(address),
            Self::Workspace(_) => None,
        }
    }
}

impl From<ComponentAddress> for ComponentReference {
    fn from(address: ComponentAddress) -> Self {
        Self::Address(address)
    }
}

impl From<WorkspaceId> for ComponentReference {
    fn from(workspace_id: WorkspaceId) -> Self {
        Self::Workspace(workspace_id)
    }
}

impl Display for ComponentReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Address(address) => write!(f, "Address({})", address),
            Self::Workspace(workspace_id) => {
                write!(f, "FromWorkspace({workspace_id})")
            },
        }
    }
}
