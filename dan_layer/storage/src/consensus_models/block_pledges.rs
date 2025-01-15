//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    hash::Hash,
};

use log::*;
use serde::{Deserialize, Serialize};
use tari_dan_common_types::{
    LockIntent,
    SubstateAddress,
    SubstateLockType,
    SubstateRequirement,
    ToSubstateAddress,
    VersionedSubstateId,
};
use tari_engine_types::substate::{SubstateId, SubstateValue};
use tari_transaction::TransactionId;

use crate::consensus_models::VersionedSubstateIdLockIntent;
pub type SubstatePledges = Vec<SubstatePledge>;

const LOG_TARGET: &str = "dan_layer::storage::consensus_models::block_pledges";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlockPledge {
    pledges: HashMap<TransactionId, Vec<VersionedSubstateIdLockIntent>>,
    substates: HashMap<SubstateAddress, SubstateValue>,
}

impl BlockPledge {
    pub fn new() -> Self {
        Self {
            pledges: HashMap::new(),
            substates: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.pledges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pledges.is_empty()
    }

    pub fn contains(&self, transaction_id: &TransactionId) -> bool {
        self.pledges.contains_key(transaction_id)
    }

    pub fn validate_integrity(&self) -> bool {
        self.pledges.iter().all(|(_tx_id, pledges)| {
            pledges.iter().all(|pledge| {
                if pledge.lock_type().is_output() {
                    return true;
                }

                let address = pledge.to_substate_address();
                if !self.substates.contains_key(&address) {
                    warn!(
                        target: LOG_TARGET,
                        "Substate not found for pledge: {}",
                        pledge.versioned_substate_id()
                    );
                    return false;
                }

                true
            })
        })
    }

    pub(crate) fn add_substate_pledge(&mut self, transaction_id: TransactionId, pledge: SubstatePledge) -> &mut Self {
        match pledge {
            SubstatePledge::Input {
                substate_id,
                is_write,
                substate,
            } => {
                self.substates.insert(substate_id.to_substate_address(), substate);

                let lock_type = if is_write {
                    SubstateLockType::Write
                } else {
                    SubstateLockType::Read
                };
                self.pledges
                    .entry(transaction_id)
                    .or_default()
                    .push(VersionedSubstateIdLockIntent::new(substate_id, lock_type, true));
            },
            SubstatePledge::Output { substate_id } => {
                self.pledges
                    .entry(transaction_id)
                    .or_default()
                    .push(VersionedSubstateIdLockIntent::new(
                        substate_id,
                        SubstateLockType::Output,
                        true,
                    ));
            },
        }
        self
    }

    pub fn remove_transaction_pledges(&mut self, transaction_id: &TransactionId) -> Option<SubstatePledges> {
        let pledges = self.pledges.remove(transaction_id)?;
        pledges
            .into_iter()
            .map(|intent| match intent.lock_type() {
                SubstateLockType::Read | SubstateLockType::Write => {
                    let is_write = intent.lock_type().is_write();
                    let substate_id = intent.into_versioned_substate_id();
                    let address = substate_id.to_substate_address();
                    let substate = match self.substates.get_mut(&address) {
                        Some(substate) => substate,
                        None => {
                            warn!(
                                target: LOG_TARGET,
                                "⚠️ Substate not found for INPUT pledge: {}",
                                substate_id
                            );
                            return None;
                        },
                    };
                    Some(SubstatePledge::Input {
                        substate_id,
                        is_write,
                        substate: substate.clone(),
                    })
                },
                SubstateLockType::Output => {
                    let substate_id = intent.into_versioned_substate_id();
                    Some(SubstatePledge::Output { substate_id })
                },
            })
            .collect()
    }

    pub fn get_transaction_pledges(&self, transaction_id: &TransactionId) -> Option<&[VersionedSubstateIdLockIntent]> {
        self.pledges.get(transaction_id).map(|v| v.as_slice())
    }

    pub fn num_substates_pledged(&self) -> usize {
        self.pledges.values().map(|s| s.len()).sum()
    }

    pub fn retain_transactions(&mut self, transaction_ids: &HashSet<TransactionId>) -> &mut Self {
        self.pledges.retain(|tx, _| transaction_ids.contains(tx));
        self
    }

    /// Returns an iterator over the pledges in the block. The pledges are randomly ordered.
    pub fn randomly_ordered_pledges_iter(
        &self,
    ) -> impl Iterator<Item = (&TransactionId, &Vec<VersionedSubstateIdLockIntent>)> {
        self.pledges.iter()
    }

    /// Returns an iterator over the substates in the block. The substates are randomly ordered.
    pub fn randomly_ordered_substates_iter(&self) -> impl Iterator<Item = (&SubstateAddress, &SubstateValue)> {
        self.substates.iter()
    }
}

impl Display for BlockPledge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (tx_id, pledges) in &self.pledges {
            write!(f, "{tx_id}:[")?;
            for pledge in pledges {
                write!(f, "{pledge}, ")?;
            }
            write!(f, "],")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubstatePledge {
    Input {
        substate_id: VersionedSubstateId,
        is_write: bool,
        substate: SubstateValue,
    },
    Output {
        substate_id: VersionedSubstateId,
    },
}

impl SubstatePledge {
    /// Returns a new SubstatePledge if it is valid, otherwise returns None
    /// A SubstatePledge is invalid if the lock type is either Write or Read and no substate value is provided.
    pub fn try_create(lock_intent: VersionedSubstateIdLockIntent, substate: Option<SubstateValue>) -> Option<Self> {
        match lock_intent.lock_type() {
            SubstateLockType::Write | SubstateLockType::Read => Some(Self::Input {
                is_write: lock_intent.lock_type().is_write(),
                substate_id: lock_intent.into_versioned_substate_id(),
                substate: substate?,
            }),
            SubstateLockType::Output => Some(Self::Output {
                substate_id: lock_intent.into_versioned_substate_id(),
            }),
        }
    }

    pub fn into_input(self) -> Option<(VersionedSubstateId, SubstateValue)> {
        match self {
            Self::Input {
                substate_id, substate, ..
            } => Some((substate_id, substate)),
            _ => None,
        }
    }

    pub fn is_output(&self) -> bool {
        matches!(self, Self::Output { .. })
    }

    pub fn is_input(&self) -> bool {
        matches!(self, Self::Input { .. })
    }

    pub fn versioned_substate_id(&self) -> &VersionedSubstateId {
        match self {
            Self::Input { substate_id, .. } => substate_id,
            Self::Output { substate_id } => substate_id,
        }
    }

    pub fn substate_id(&self) -> &SubstateId {
        self.versioned_substate_id().substate_id()
    }

    pub fn as_substate_lock_type(&self) -> SubstateLockType {
        match self {
            Self::Input { is_write, .. } => {
                if *is_write {
                    SubstateLockType::Write
                } else {
                    SubstateLockType::Read
                }
            },
            Self::Output { .. } => SubstateLockType::Output,
        }
    }

    pub fn to_substate_address(&self) -> SubstateAddress {
        self.versioned_substate_id().to_substate_address()
    }

    pub fn satisfies_requirement(&self, req: &SubstateRequirement) -> bool {
        // Check if a requirement is met by this pledge. If the requirement does not specify a version, then the version
        // requirement is, by definition, met.
        req.version.map_or(true, |v| v == self.versioned_substate_id().version) &&
            self.substate_id() == req.substate_id()
    }

    pub fn satisfies_substate_and_version(&self, substate_id: &SubstateId, version: u32) -> bool {
        self.versioned_substate_id().version == version && self.substate_id() == substate_id
    }

    pub fn satisfies_lock_intent<T: LockIntent>(&self, lock_intent: T) -> bool {
        if lock_intent.version_to_lock() != self.versioned_substate_id().version() {
            return false;
        }
        let lock_type = self.as_substate_lock_type();
        if !lock_type.allows(lock_intent.lock_type()) {
            return false;
        }

        if lock_intent.substate_id() != self.substate_id() {
            return false;
        }
        true
    }
}

/// These are to detect and prevent duplicates in pledging.
impl Hash for SubstatePledge {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.substate_id().hash(state);
        self.versioned_substate_id().version.hash(state);
        self.as_substate_lock_type().hash(state);
    }
}

impl PartialEq for SubstatePledge {
    fn eq(&self, other: &Self) -> bool {
        self.as_substate_lock_type() == other.as_substate_lock_type() &&
            self.versioned_substate_id() == other.versioned_substate_id()
    }
}

impl Eq for SubstatePledge {}

impl Display for SubstatePledge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubstatePledge::Input {
                substate_id, is_write, ..
            } => {
                if *is_write {
                    write!(f, "Write: {}", substate_id)
                } else {
                    write!(f, "Read: {}", substate_id)
                }
            },
            SubstatePledge::Output { substate_id } => write!(f, "Output: {}", substate_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use tari_engine_types::component::{ComponentBody, ComponentHeader};
    use tari_template_lib::{
        auth::ComponentAccessRules,
        models::{ComponentAddress, EntityId},
    };

    use super::*;

    fn create_substate_id(seed: u8) -> VersionedSubstateId {
        VersionedSubstateId::new(SubstateId::Component(ComponentAddress::from_array([seed; 32])), 0)
    }

    #[test]
    fn basic() {
        let mut pledge = BlockPledge::new();
        let tx_id = TransactionId::default();
        let substate_value = SubstateValue::Component(ComponentHeader {
            template_address: Default::default(),
            module_name: "".to_string(),
            owner_key: None,
            owner_rule: Default::default(),
            access_rules: ComponentAccessRules::allow_all(),
            entity_id: EntityId::from_array([1u8; 20]),
            body: ComponentBody::empty(),
        });
        let substate_id = create_substate_id(0);
        let pledge = pledge.add_substate_pledge(tx_id, SubstatePledge::Input {
            substate_id: substate_id.clone(),
            is_write: true,
            substate: substate_value.clone(),
        });
        assert_eq!(pledge.len(), 1);
        assert_eq!(pledge.num_substates_pledged(), 1);
        assert!(pledge.contains(&tx_id));
        assert_eq!(pledge.get_transaction_pledges(&tx_id).unwrap().len(), 1);
    }
}
