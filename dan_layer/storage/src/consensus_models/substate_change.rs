//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tari_dan_common_types::{shard::Shard, SubstateAddress, ToSubstateAddress, VersionedSubstateId};
use tari_engine_types::substate::{Substate, SubstateId};
use tari_state_tree::SubstateTreeChange;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubstateChange {
    Up {
        id: VersionedSubstateId,
        shard: Shard,
        substate: Substate,
    },
    Down {
        id: VersionedSubstateId,
        shard: Shard,
    },
}

impl SubstateChange {
    pub fn to_substate_address(&self) -> SubstateAddress {
        match self {
            SubstateChange::Up { id, .. } => id.to_substate_address(),
            SubstateChange::Down { id, .. } => id.to_substate_address(),
        }
    }

    pub fn versioned_substate_id(&self) -> &VersionedSubstateId {
        match self {
            SubstateChange::Up { id, .. } => id,
            SubstateChange::Down { id, .. } => id,
        }
    }

    pub fn substate_id(&self) -> &SubstateId {
        self.versioned_substate_id().substate_id()
    }

    pub fn version(&self) -> u32 {
        self.versioned_substate_id().version()
    }

    pub fn substate(&self) -> Option<&Substate> {
        match self {
            SubstateChange::Up { substate, .. } => Some(substate),
            _ => None,
        }
    }

    pub fn shard(&self) -> Shard {
        match self {
            SubstateChange::Up { shard, .. } => *shard,
            SubstateChange::Down { shard, .. } => *shard,
        }
    }

    pub fn is_down(&self) -> bool {
        matches!(self, SubstateChange::Down { .. })
    }

    pub fn is_up(&self) -> bool {
        matches!(self, SubstateChange::Up { .. })
    }

    pub fn up_substate(&self) -> Option<&Substate> {
        match self {
            SubstateChange::Up { substate, .. } => Some(substate),
            _ => None,
        }
    }

    pub fn into_up(self) -> Option<Substate> {
        match self {
            SubstateChange::Up { substate: value, .. } => Some(value),
            _ => None,
        }
    }

    pub fn as_change_string(&self) -> &'static str {
        match self {
            SubstateChange::Up { .. } => "Up",
            SubstateChange::Down { .. } => "Down",
        }
    }
}

impl Display for SubstateChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubstateChange::Up { id, shard, substate } => {
                write!(f, "Up: {}, {}, substate hash: {}", id, shard, substate.to_value_hash())
            },
            SubstateChange::Down { id, shard } => write!(f, "Down: {}, {}", id, shard),
        }
    }
}

impl From<&SubstateChange> for SubstateTreeChange {
    fn from(value: &SubstateChange) -> Self {
        match value {
            SubstateChange::Up { id, substate, .. } => SubstateTreeChange::Up {
                id: id.clone(),
                value_hash: substate.to_value_hash(),
            },
            SubstateChange::Down { id, .. } => SubstateTreeChange::Down { id: id.clone() },
        }
    }
}
