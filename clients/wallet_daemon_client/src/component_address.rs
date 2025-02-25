//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    convert::Infallible,
    fmt::{Display, Formatter},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use tari_template_lib::models::ComponentAddress;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
pub enum ComponentAddressOrName {
    ComponentAddress(ComponentAddress),
    Name(String),
}

impl ComponentAddressOrName {
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::ComponentAddress(_) => None,
            Self::Name(name) => Some(name),
        }
    }

    pub fn component_address(&self) -> Option<&ComponentAddress> {
        match self {
            Self::ComponentAddress(address) => Some(address),
            Self::Name(_) => None,
        }
    }
}

impl Display for ComponentAddressOrName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ComponentAddress(address) => write!(f, "{}", address),
            Self::Name(name) => write!(f, "{}", name),
        }
    }
}

impl FromStr for ComponentAddressOrName {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(address) = ComponentAddress::from_str(s) {
            Ok(Self::ComponentAddress(address))
        } else {
            Ok(Self::Name(s.to_string()))
        }
    }
}

impl From<ComponentAddress> for ComponentAddressOrName {
    fn from(address: ComponentAddress) -> Self {
        Self::ComponentAddress(address)
    }
}

impl From<String> for ComponentAddressOrName {
    fn from(name: String) -> Self {
        Self::Name(name)
    }
}
