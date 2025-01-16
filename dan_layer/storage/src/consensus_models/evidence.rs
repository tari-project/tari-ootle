//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use borsh::BorshSerialize;
use indexmap::IndexMap;
use log::*;
use serde::{Deserialize, Serialize};
use tari_dan_common_types::{
    borsh::indexmap as indexmap_borsh,
    option::DisplayContainer,
    LockIntent,
    NumPreshards,
    ShardGroup,
    SubstateRequirement,
    ToSubstateAddress,
    VersionedSubstateId,
};
use tari_engine_types::{serde_with, substate::SubstateId};

use crate::consensus_models::QcId;

const LOG_TARGET: &str = "tari::dan::consensus_models::evidence";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default, BorshSerialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct Evidence {
    // Serialize JSON as an array of objects since ShardGroup is a non-string key
    #[serde(with = "serde_with::vec")]
    #[cfg_attr(feature = "ts", ts(type = "Array<[any, any]>"))]
    #[borsh(serialize_with = "indexmap_borsh::serialize")]
    evidence: IndexMap<ShardGroup, ShardGroupEvidence>,
}

impl Evidence {
    pub fn empty() -> Self {
        Self {
            evidence: IndexMap::new(),
        }
    }

    pub fn from_initial_substates<I, O>(num_preshards: NumPreshards, num_committees: u32, inputs: I, outputs: O) -> Self
    where
        I: IntoIterator<Item = SubstateRequirement>,
        O: IntoIterator<Item = VersionedSubstateId>,
    {
        let mut evidence = Self::empty();

        for obj in inputs {
            // Version does not affect the shard group
            let substate_address = obj.to_substate_address_zero_version();
            let sg = substate_address.to_shard_group(num_preshards, num_committees);
            evidence.add_shard_group(sg).insert_empty_input(obj.into_substate_id());
        }

        for obj in outputs {
            let substate_address = obj.to_substate_address();
            let sg = substate_address.to_shard_group(num_preshards, num_committees);
            evidence.add_shard_group(sg).insert_output(obj.substate_id, obj.version);
        }

        evidence.evidence.sort_keys();

        evidence
    }

    pub fn from_inputs_and_outputs<I, O, L1, L2>(
        num_preshards: NumPreshards,
        num_committees: u32,
        resolved_inputs: I,
        resulting_outputs: O,
    ) -> Self
    where
        L1: LockIntent,
        I: IntoIterator<Item = L1>,
        L2: LockIntent,
        O: IntoIterator<Item = L2>,
    {
        let mut evidence = Self::empty();

        for obj in resolved_inputs {
            let substate_address = obj.to_substate_address();
            let sg = substate_address.to_shard_group(num_preshards, num_committees);
            evidence.add_shard_group(sg).insert_from_lock_intent(obj);
        }

        for obj in resulting_outputs {
            let substate_address = obj.to_substate_address();
            let sg = substate_address.to_shard_group(num_preshards, num_committees);
            evidence.add_shard_group(sg).insert_from_lock_intent(obj);
        }

        evidence.evidence.sort_keys();

        evidence
    }

    pub fn to_includes_only_shard_group(&self, shard_group: ShardGroup) -> Self {
        let mut evidence = Self::empty();
        if let Some(ev) = self.get(&shard_group) {
            *evidence.add_shard_group(shard_group) = ev.clone();
        }
        evidence
    }

    pub fn all_inputs_iter(&self) -> impl Iterator<Item = (&ShardGroup, &SubstateId, &Option<EvidenceInputLockData>)> {
        self.evidence.iter().flat_map(|(sg, evidence)| {
            evidence
                .inputs
                .iter()
                .map(move |(substate_id, lock)| (sg, substate_id, lock))
        })
    }

    pub fn all_outputs_iter(&self) -> impl Iterator<Item = (&ShardGroup, &SubstateId, &u32)> {
        self.evidence.iter().flat_map(|(sg, evidence)| {
            evidence
                .outputs
                .iter()
                .map(move |(substate_id, version)| (sg, substate_id, version))
        })
    }

    pub fn all_objects_accepted(&self) -> bool {
        // CASE: all inputs and outputs are accept justified. If they have been accept justified, they have implicitly
        // been prepare justified. This may happen if the local node is only involved in outputs (and therefore
        // sequences using the LocalAccept foreign proposal)
        self.evidence.values().all(|e| e.is_accept_justified())
    }

    pub fn all_shard_groups_prepared(&self) -> bool {
        self.evidence
            .values()
            // CASE: we use prepare OR accept because inputs can only be accept justified if they were prepared. Prepared
            // may be implicit (null) if the local node is only involved in outputs (and therefore sequences using the LocalAccept
            // foreign proposal)
            .all(|e| e.is_prepare_justified() || e.is_accept_justified())
    }

    pub fn all_input_shard_groups_prepared(&self) -> bool {
        self.evidence
            .values()
            .filter(|e| {
                // CASE: we only require input shard groups to prepare
                 !e.inputs().is_empty()
            })
            // CASE: we use prepare OR accept because inputs can only be accept justified if they were prepared. Prepared
            // may be implicit (null) if the local node is only involved in outputs (and therefore sequences using the LocalAccept
            // foreign proposal). If there are no
            .all(|e| e.is_prepare_justified() || e.is_accept_justified())
    }

    /// Returns true if all substates in the given shard group are output locks.
    /// This assumes the provided evidence is complete before this is called.
    /// If no evidence is present for the shard group, false is returned.
    pub fn is_committee_output_only(&self, shard_group: ShardGroup) -> bool {
        self.evidence.get(&shard_group).map_or(true, |e| e.inputs().is_empty())
    }

    pub fn is_empty(&self) -> bool {
        self.evidence.is_empty()
    }

    pub fn len(&self) -> usize {
        self.evidence.len()
    }

    pub fn get(&self, shard_group: &ShardGroup) -> Option<&ShardGroupEvidence> {
        self.evidence.get(shard_group)
    }

    pub fn get_mut(&mut self, shard_group: &ShardGroup) -> Option<&mut ShardGroupEvidence> {
        self.evidence.get_mut(shard_group)
    }

    pub fn has(&self, shard_group: &ShardGroup) -> bool {
        self.evidence.contains_key(shard_group)
    }

    pub fn has_and_not_empty(&self, shard_group: &ShardGroup) -> bool {
        self.evidence
            .get(shard_group)
            .is_some_and(|e| !e.inputs.is_empty() || !e.outputs.is_empty())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ShardGroup, &ShardGroupEvidence)> {
        self.evidence.iter()
    }

    pub fn contains(&self, shard_group: &ShardGroup) -> bool {
        self.evidence.contains_key(shard_group)
    }

    pub fn qc_ids_iter(&self) -> impl Iterator<Item = &QcId> + '_ {
        self.evidence
            .values()
            .flat_map(|e| e.prepare_qc.iter().chain(e.accept_qc.iter()))
    }

    pub fn add_shard_group(&mut self, shard_group: ShardGroup) -> &mut ShardGroupEvidence {
        self.evidence.entry(shard_group).or_default()
    }

    pub fn shard_groups_iter(&self) -> impl Iterator<Item = &ShardGroup> {
        self.evidence.keys()
    }

    pub fn missing_evidence_iter(&self) -> impl Iterator<Item = &ShardGroup> {
        self.evidence.iter().filter_map(|(sg, e)| {
            if e.prepare_qc.is_none() || e.accept_qc.is_none() {
                Some(sg)
            } else {
                None
            }
        })
    }

    pub fn num_shard_groups(&self) -> usize {
        self.evidence.len()
    }

    /// Add or update shard groups, substates and locks into Evidence. Existing prepare/accept QC IDs are not changed.
    pub fn update(&mut self, other: &Evidence) -> &mut Self {
        for (sg, evidence) in other.iter() {
            let evidence_mut = self.evidence.entry(*sg).or_default();
            let inputs_mut = &mut evidence_mut.inputs;

            for (substate_id, evidence) in evidence.inputs.iter().map(|(id, lock)| (id.clone(), *lock)) {
                if let Some(e_mut) = inputs_mut.get_mut(&substate_id) {
                    match evidence {
                        Some(e) => match e_mut {
                            Some(e_mut) => {
                                e_mut.is_write = e.is_write;
                                e_mut.version = e.version;
                            },
                            None => {
                                *e_mut = Some(e);
                            },
                        },
                        None => continue,
                    }
                } else {
                    inputs_mut.insert(substate_id, evidence);
                }
            }
            evidence_mut
                .outputs
                .extend(evidence.outputs.iter().map(|(id, version)| (id.clone(), *version)));
            evidence_mut.sort_substates();
        }
        self.evidence.sort_keys();
        self
    }

    pub fn eq_pledges(&self, other: &Evidence) -> bool {
        if self.len() != other.len() {
            debug!(
                target: LOG_TARGET,
                "Evidence length mismatch: self={}, other={}",
                self.len(),
                other.len()
            );
            return false;
        }

        for (sg, evidence) in self.iter() {
            if let Some(other_evidence) = other.get(sg) {
                if evidence.inputs() != other_evidence.inputs() {
                    debug!(
                        target: LOG_TARGET,
                        "Inputs mismatch for shard group {}: self={:?}, other={:?}",
                        sg,
                        evidence.inputs(),
                        other_evidence.inputs()
                    );
                    return false;
                }
                if evidence.outputs() != other_evidence.outputs() {
                    debug!(
                        target: LOG_TARGET,
                        "Outputs mismatch for shard group {}: self={:?}, other={:?}",
                        sg,
                        evidence.outputs(),
                        other_evidence.outputs()
                    );
                    return false;
                }
            } else {
                debug!(target: LOG_TARGET, "Missing shard group evidence for {}", sg);
                return false;
            }
        }
        true
    }
}

impl FromIterator<(ShardGroup, ShardGroupEvidence)> for Evidence {
    fn from_iter<T: IntoIterator<Item = (ShardGroup, ShardGroupEvidence)>>(iter: T) -> Self {
        let mut evidence = iter.into_iter().collect::<IndexMap<_, _>>();
        evidence.sort_keys();
        Evidence { evidence }
    }
}

impl Display for Evidence {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            if self.is_empty() {
                write!(f, "{{EMPTY}}")?;
                return Ok(());
            }

            for (i, (shard_group, shard_evidence)) in self.evidence.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                match (
                    shard_evidence.is_prepare_justified(),
                    shard_evidence.is_accept_justified(),
                ) {
                    (_, true) => write!(f, "{}: ACCEPTED", shard_group)?,
                    (true, false) => write!(f, "{}: PREPARED", shard_group)?,
                    _ => write!(f, "{}: NO EVIDENCE", shard_group)?,
                }
            }
        } else {
            if self.is_empty() {
                write!(f, "{{EMPTY}}")?;
                return Ok(());
            }
            write!(f, "{{")?;
            for (i, (substate_address, shard_evidence)) in self.evidence.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}: {}", substate_address, shard_evidence)?;
            }
            write!(f, "}}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default, BorshSerialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct ShardGroupEvidence {
    #[borsh(serialize_with = "indexmap_borsh::serialize")]
    #[cfg_attr(feature = "ts", ts(type = "Record<string, any>"))]
    inputs: IndexMap<SubstateId, Option<EvidenceInputLockData>>,
    #[borsh(serialize_with = "indexmap_borsh::serialize")]
    #[cfg_attr(feature = "ts", ts(type = "Record<string, number>"))]
    outputs: IndexMap<SubstateId, u32>,
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    prepare_qc: Option<QcId>,
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    accept_qc: Option<QcId>,
}

impl ShardGroupEvidence {
    pub fn insert_from_lock_intent<T: LockIntent>(&mut self, lock: T) -> &mut Self {
        if lock.lock_type().is_input() {
            self.inputs.insert_sorted(
                lock.substate_id().clone(),
                Some(EvidenceInputLockData {
                    is_write: lock.lock_type().is_write(),
                    version: lock.version_to_lock(),
                }),
            );
        } else {
            self.outputs
                .insert_sorted(lock.substate_id().clone(), lock.version_to_lock());
        }
        self
    }

    pub fn insert_empty_input(&mut self, substate_id: SubstateId) -> &mut Self {
        self.inputs.insert_sorted(substate_id, None);
        self
    }

    pub fn insert_output(&mut self, substate_id: SubstateId, version: u32) -> &mut Self {
        self.outputs.insert_sorted(substate_id, version);
        self
    }

    pub fn is_prepare_justified(&self) -> bool {
        self.prepare_qc.is_some()
    }

    pub fn is_accept_justified(&self) -> bool {
        self.accept_qc.is_some()
    }

    pub fn is_all_inputs_pledged(&self) -> bool {
        self.inputs.iter().all(|(_, lock)| lock.is_some())
    }

    pub fn inputs(&self) -> &IndexMap<SubstateId, Option<EvidenceInputLockData>> {
        &self.inputs
    }

    pub fn outputs(&self) -> &IndexMap<SubstateId, u32> {
        &self.outputs
    }

    fn sort_substates(&mut self) {
        self.inputs.sort_keys();
        self.outputs.sort_keys();
    }

    pub fn contains_pledge(&self, substate_id: &SubstateId, version: u32, is_input: bool) -> bool {
        if is_input {
            return self
                .inputs
                .get(substate_id)
                .is_some_and(|e| e.as_ref().is_some_and(|e| e.version == version));
        }

        self.outputs.get(substate_id).is_some_and(|v| *v == version)
    }

    pub fn update(&mut self, other: &ShardGroupEvidence) -> &mut Self {
        for (substate_id, lock) in &other.inputs {
            if let Some(e) = lock {
                if let Some(ev_mut) = self.inputs.get_mut(substate_id) {
                    *ev_mut = Some(*e);
                } else {
                    self.inputs.insert_sorted(substate_id.clone(), Some(*e));
                }
            } else if !self.inputs.contains_key(substate_id) {
                self.inputs.insert_sorted(substate_id.clone(), None);
            } else {
                // Do nothing
            }
        }
        for (substate_id, version) in &other.outputs {
            if let Some(v) = self.outputs.get_mut(substate_id) {
                *v = *version;
            } else {
                self.outputs.insert_sorted(substate_id.clone(), *version);
            }
        }
        self
    }

    pub fn set_prepare_qc(&mut self, qc_id: QcId) -> &mut Self {
        debug!(
            target: LOG_TARGET,
            "set_prepare_qc: QC[{qc_id}]",
        );
        self.prepare_qc = Some(qc_id);
        self
    }

    pub fn set_accept_qc(&mut self, qc_id: QcId) -> &mut Self {
        debug!(
            target: LOG_TARGET,
            "set_accept_qc: QC[{qc_id}]",
        );
        self.accept_qc = Some(qc_id);
        self
    }
}

impl Display for ShardGroupEvidence {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "inputs[")?;
        for (i, (substate_id, lock)) in self.inputs.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}: {}", substate_id, lock.display())?;
        }
        write!(f, "],")?;
        write!(f, "outputs[")?;
        for (i, (substate_id, version)) in self.outputs.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}: {}", substate_id, version)?;
        }
        write!(f, "]")?;
        if let Some(qc_id) = self.prepare_qc {
            write!(f, " Prepare[{}]", qc_id)?;
        } else {
            write!(f, " Prepare[NONE]")?;
        }
        if let Some(qc_id) = self.accept_qc {
            write!(f, " Accept[{}]", qc_id)?;
        } else {
            write!(f, " Accept[NONE]")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, BorshSerialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct EvidenceInputLockData {
    pub is_write: bool,
    pub version: u32,
}

impl Display for EvidenceInputLockData {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let rw = if self.is_write { "Write" } else { "Read" };
        write!(f, "v{} {}", self.version, rw)
    }
}

#[cfg(test)]
mod tests {
    use tari_dan_common_types::SubstateLockType;
    use tari_template_lib::models::{ComponentAddress, ObjectKey};

    use super::*;
    use crate::consensus_models::SubstateRequirementLockIntent;

    fn seed_substate_id(seed: u8) -> SubstateId {
        SubstateId::Component(ComponentAddress::from_array([seed; ObjectKey::LENGTH]))
    }

    fn seed_lock_intent(seed: u8, ty: SubstateLockType) -> SubstateRequirementLockIntent {
        SubstateRequirementLockIntent::new(seed_substate_id(seed), 0, ty)
    }

    #[test]
    fn it_merges_two_evidences_together() {
        let sg1 = ShardGroup::new(0, 1);
        let sg2 = ShardGroup::new(2, 3);
        let sg3 = ShardGroup::new(4, 5);

        let mut evidence1 = Evidence::empty();
        evidence1
            .add_shard_group(sg1)
            .insert_from_lock_intent(seed_lock_intent(1, SubstateLockType::Write));
        evidence1
            .add_shard_group(sg1)
            .insert_from_lock_intent(seed_lock_intent(2, SubstateLockType::Read));

        let mut evidence2 = Evidence::empty();
        evidence2
            .add_shard_group(sg1)
            .insert_from_lock_intent(seed_lock_intent(2, SubstateLockType::Write));
        evidence2
            .add_shard_group(sg1)
            .insert_from_lock_intent(seed_lock_intent(2, SubstateLockType::Output));
        evidence2
            .add_shard_group(sg2)
            .insert_from_lock_intent(seed_lock_intent(3, SubstateLockType::Output));
        evidence2
            .add_shard_group(sg3)
            .insert_from_lock_intent(seed_lock_intent(4, SubstateLockType::Output));

        evidence1.update(&evidence2);

        assert_eq!(evidence1.len(), 3);
        assert!(
            evidence1
                .get(&sg1)
                .unwrap()
                .inputs
                .get(&seed_substate_id(1))
                .unwrap()
                .unwrap()
                .is_write,
        );
        assert!(
            evidence1
                .get(&sg1)
                .unwrap()
                .inputs
                .get(&seed_substate_id(2))
                .unwrap()
                .unwrap()
                .is_write,
        );
        assert_eq!(
            evidence1.get(&sg1).unwrap().outputs.get(&seed_substate_id(2)),
            Some(&0u32)
        );
        assert_eq!(
            evidence1.get(&sg1).unwrap().outputs.get(&seed_substate_id(2)),
            Some(&0u32)
        );
    }
}
