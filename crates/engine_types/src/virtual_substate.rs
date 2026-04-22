//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
    ops::Deref,
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use tari_template_lib::types::Hash32;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VirtualSubstateId {
    CurrentEpoch,
    CurrentEpochHash,
}

impl Display for VirtualSubstateId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            VirtualSubstateId::CurrentEpoch => write!(f, "Virtual(CurrentEpoch)"),
            VirtualSubstateId::CurrentEpochHash => write!(f, "Virtual(CurrentEpochHash)"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VirtualSubstate {
    CurrentEpoch(u64),
    CurrentEpochHash(Hash32),
}

/// Read-only Virtual substate collection. THis collection is cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct VirtualSubstates(Arc<HashMap<VirtualSubstateId, VirtualSubstate>>);

impl VirtualSubstates {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn current_epoch(&self) -> Option<u64> {
        match self.get(&VirtualSubstateId::CurrentEpoch) {
            Some(VirtualSubstate::CurrentEpoch(epoch)) => Some(*epoch),
            _ => None,
        }
    }

    pub fn current_epoch_hash(&self) -> Option<Hash32> {
        match self.get(&VirtualSubstateId::CurrentEpochHash) {
            Some(VirtualSubstate::CurrentEpochHash(hash)) => Some(*hash),
            _ => None,
        }
    }
}

impl Deref for VirtualSubstates {
    type Target = HashMap<VirtualSubstateId, VirtualSubstate>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromIterator<(VirtualSubstateId, VirtualSubstate)> for VirtualSubstates {
    fn from_iter<T: IntoIterator<Item = (VirtualSubstateId, VirtualSubstate)>>(iter: T) -> Self {
        Self(Arc::new(iter.into_iter().collect()))
    }
}

impl From<HashMap<VirtualSubstateId, VirtualSubstate>> for VirtualSubstates {
    fn from(map: HashMap<VirtualSubstateId, VirtualSubstate>) -> Self {
        Self(Arc::new(map))
    }
}
