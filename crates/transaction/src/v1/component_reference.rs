//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use tari_template_lib_types::ComponentAddress;

use crate::args::WorkspaceId;

/// A reference to a component, either by its address or by a workspace ID.
#[derive(Debug, Clone, PartialEq, borsh::BorshSerialize, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ComponentReference {
    #[n(0)]
    Address(#[n(0)] ComponentAddress),
    #[n(1)]
    Workspace(#[n(0)] WorkspaceId),
}

impl ComponentReference {
    pub fn address(&self) -> Option<&ComponentAddress> {
        match self {
            Self::Address(address) => Some(address),
            Self::Workspace(_) => None,
        }
    }

    /// Shift any workspace ID by the given amount. Used when merging transaction builders.
    pub fn remap_workspace_id(&mut self, id_offset: WorkspaceId) {
        if let Self::Workspace(id) = self {
            *id = id.checked_add(id_offset).expect("Workspace ID overflow during merge");
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
