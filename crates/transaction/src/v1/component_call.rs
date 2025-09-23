//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use tari_bor::{Deserialize, Serialize};
use tari_template_lib::models::ComponentAddress;

use crate::args::WorkspaceId;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ComponentCall {
    Address(ComponentAddress),
    Workspace(WorkspaceId),
}

impl From<ComponentAddress> for ComponentCall {
    fn from(address: ComponentAddress) -> Self {
        Self::Address(address)
    }
}

impl From<WorkspaceId> for ComponentCall {
    fn from(workspace_id: WorkspaceId) -> Self {
        Self::Workspace(workspace_id)
    }
}

impl Display for ComponentCall {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ComponentCall::Address(address) => write!(f, "Address({})", address),
            ComponentCall::Workspace(workspace_id) => {
                write!(f, "FromWorkspace({workspace_id})")
            },
        }
    }
}
