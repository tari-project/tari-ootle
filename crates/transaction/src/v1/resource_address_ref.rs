//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use tari_bor::{Deserialize, Serialize};
use tari_template_lib_types::ResourceAddress;

use crate::args::{WorkspaceId, WorkspaceOffsetId};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ResourceAddressRef {
    Address(ResourceAddress),
    Workspace(WorkspaceOffsetId),
}

impl From<ResourceAddress> for ResourceAddressRef {
    fn from(address: ResourceAddress) -> Self {
        Self::Address(address)
    }
}

impl From<WorkspaceOffsetId> for ResourceAddressRef {
    fn from(workspace_id: WorkspaceOffsetId) -> Self {
        Self::Workspace(workspace_id)
    }
}

impl From<WorkspaceId> for ResourceAddressRef {
    fn from(workspace_id: WorkspaceId) -> Self {
        WorkspaceOffsetId::new(workspace_id).into()
    }
}

impl Display for ResourceAddressRef {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceAddressRef::Address(address) => write!(f, "Address({})", address),
            ResourceAddressRef::Workspace(workspace_id) => {
                write!(f, "FromWorkspace({workspace_id})")
            },
        }
    }
}
