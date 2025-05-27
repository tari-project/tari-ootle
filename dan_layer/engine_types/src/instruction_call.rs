//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use tari_bor::{Deserialize, Serialize};
use tari_template_lib::{args::WorkspaceKey, models::ComponentAddress};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub enum ComponentCall {
    Address(ComponentAddress),
    FromWorkspace(WorkspaceKey),
}

impl From<ComponentAddress> for ComponentCall {
    fn from(address: ComponentAddress) -> Self {
        Self::Address(address)
    }
}

impl From<WorkspaceKey> for ComponentCall {
    fn from(workspace_key: WorkspaceKey) -> Self {
        Self::FromWorkspace(workspace_key)
    }
}

impl Display for ComponentCall {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ComponentCall::Address(address) => write!(f, "Address({})", address),
            ComponentCall::FromWorkspace(workspace_key) => {
                write!(f, "FromWorkspace({})", String::from_utf8_lossy(workspace_key))
            },
        }
    }
}
