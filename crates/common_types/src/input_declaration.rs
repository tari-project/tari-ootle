//    Copyright 2026 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{borrow::Borrow, fmt::Display, str::FromStr};

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use tari_engine_types::substate::SubstateId;

use crate::{
    NumPreshards,
    SubstateAddress,
    SubstateLockType,
    SubstateRequirement,
    SubstateRequirementRef,
    VersionedSubstateId,
    shard::Shard,
};

/// One input substate a transaction declares, and the access it intends to take on it.
///
/// A shard group that does not hold an input cannot execute to discover how it is used, so it locks
/// from the declaration alone. `is_write` is what lets that lock be a read lock: read-declared
/// inputs admit concurrent readers, write-declared inputs exclude everything. Writing to an input
/// declared read aborts the transaction in the engine, so the declaration is an upper bound on
/// access rather than a hint.
#[derive(
    Debug, Clone, Deserialize, Serialize, borsh::BorshSerialize, minicbor::Encode, minicbor::Decode, minicbor::CborLen,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct InputDeclaration {
    #[n(0)]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub substate_id: SubstateId,
    #[n(1)]
    pub version: Option<u32>,
    /// Whether the transaction intends to write to this input. A JSON declaration that omits it is
    /// taken as a write: an over-declaring transaction locks more than it needs and runs slowly,
    /// where an under-declaring one would abort part-way through execution. The CBOR encoding is
    /// the consensus form and requires it, so a peer cannot have its intent guessed for it.
    #[n(2)]
    #[serde(default = "write_by_default")]
    pub is_write: bool,
}

const fn write_by_default() -> bool {
    true
}

impl InputDeclaration {
    pub const fn new(substate_id: SubstateId, version: Option<u32>, is_write: bool) -> Self {
        Self {
            substate_id,
            version,
            is_write,
        }
    }

    /// A write declaration, the conservative default for a caller that does not know.
    pub fn write<T: Into<SubstateId>>(id: T) -> Self {
        Self::new(id.into(), None, true)
    }

    pub fn write_versioned<T: Into<SubstateId>>(id: T, version: u32) -> Self {
        Self::new(id.into(), Some(version), true)
    }

    /// A read declaration. The engine aborts the transaction if it writes to this input.
    pub fn read<T: Into<SubstateId>>(id: T) -> Self {
        Self::new(id.into(), None, false)
    }

    pub fn read_versioned<T: Into<SubstateId>>(id: T, version: u32) -> Self {
        Self::new(id.into(), Some(version), false)
    }

    pub fn substate_id(&self) -> &SubstateId {
        &self.substate_id
    }

    pub fn into_substate_id(self) -> SubstateId {
        self.substate_id
    }

    pub fn version(&self) -> Option<u32> {
        self.version
    }

    pub fn is_write(&self) -> bool {
        self.is_write
    }

    pub fn is_read(&self) -> bool {
        !self.is_write
    }

    pub fn lock_type(&self) -> SubstateLockType {
        if self.is_write {
            SubstateLockType::Write
        } else {
            SubstateLockType::Read
        }
    }

    pub fn with_intent(mut self, is_write: bool) -> Self {
        self.is_write = is_write;
        self
    }

    pub fn with_version(self, version: u32) -> VersionedSubstateId {
        VersionedSubstateId::new(self.substate_id, version)
    }

    pub fn to_substate_requirement(&self) -> SubstateRequirement {
        SubstateRequirement::new(self.substate_id.clone(), self.version)
    }

    pub fn into_substate_requirement(self) -> SubstateRequirement {
        SubstateRequirement::new(self.substate_id, self.version)
    }

    pub fn as_ref(&self) -> InputDeclarationRef<'_> {
        InputDeclarationRef {
            substate_id: &self.substate_id,
            version: self.version,
            is_write: self.is_write,
        }
    }

    pub fn to_substate_address(&self) -> Option<SubstateAddress> {
        self.version
            .map(|v| SubstateAddress::from_substate_id(&self.substate_id, v))
    }

    pub fn to_shard(&self, num_shards: NumPreshards) -> Option<Shard> {
        self.to_substate_requirement().to_shard(num_shards)
    }
}

/// Every `SubstateId`-like value declares a write, because a caller that has not said otherwise has
/// not told us it only reads.
impl<T: Into<SubstateId>> From<T> for InputDeclaration {
    fn from(value: T) -> Self {
        Self::write(value)
    }
}

impl From<SubstateRequirement> for InputDeclaration {
    fn from(value: SubstateRequirement) -> Self {
        Self::new(value.substate_id, value.version, true)
    }
}

impl From<VersionedSubstateId> for InputDeclaration {
    fn from(value: VersionedSubstateId) -> Self {
        let version = value.version();
        Self::new(value.into_substate_id(), Some(version), true)
    }
}

impl From<InputDeclaration> for SubstateRequirement {
    fn from(value: InputDeclaration) -> Self {
        value.into_substate_requirement()
    }
}

/// `read` and `write` suffix the `<id>[:<version>]` form that [`SubstateRequirement`] parses, and a
/// `?` version parses as unversioned so that [`Display`] round-trips.
impl FromStr for InputDeclaration {
    type Err = InputDeclarationParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || InputDeclarationParseError(s.to_string());
        let mut parts = s.split(':');

        let address = parts.next().ok_or_else(err)?;
        let substate_id = SubstateId::from_str(address).map_err(|_| err())?;

        let mut version = None;
        let mut is_write = true;
        let mut has_version_part = false;

        if let Some(part) = parts.next() {
            match part {
                "read" => is_write = false,
                "write" => {},
                "?" => has_version_part = true,
                v => {
                    version = Some(v.parse().map_err(|_| err())?);
                    has_version_part = true;
                },
            }

            // An intent only follows a version, so a second part after an intent is malformed.
            if let Some(part) = parts.next() {
                if !has_version_part {
                    return Err(err());
                }
                match part {
                    "read" => is_write = false,
                    "write" => {},
                    _ => return Err(err()),
                }
            }
        }

        if parts.next().is_some() {
            return Err(err());
        }

        Ok(Self::new(substate_id, version, is_write))
    }
}

impl Display for InputDeclaration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.version {
            Some(v) => write!(f, "{}:{}", self.substate_id, v)?,
            None => write!(f, "{}:?", self.substate_id)?,
        }
        if !self.is_write {
            write!(f, ":read")?;
        }
        Ok(())
    }
}

/// The declared intent is not part of a declaration's identity: a transaction names a substate at
/// most once, so two declarations of the same substate are the same declaration however they differ
/// on version or intent.
impl PartialEq for InputDeclaration {
    fn eq(&self, other: &Self) -> bool {
        self.substate_id == other.substate_id
    }
}

impl Eq for InputDeclaration {}

impl std::hash::Hash for InputDeclaration {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.substate_id.hash(state);
    }
}

impl Borrow<SubstateId> for InputDeclaration {
    fn borrow(&self) -> &SubstateId {
        &self.substate_id
    }
}

impl AsRef<SubstateId> for InputDeclaration {
    fn as_ref(&self) -> &SubstateId {
        &self.substate_id
    }
}

/// Adds `decl` to a transaction's declarations, widening the intent if the substate is already
/// declared.
///
/// A substate that a transaction both reads and writes must be write-locked, and because a set
/// keys declarations on the substate id alone, plain insertion would let declaration order decide
/// that. The version of an existing declaration is left as it is.
pub fn declare_input(inputs: &mut IndexSet<InputDeclaration>, decl: InputDeclaration) {
    let widened = match inputs.get(decl.substate_id()) {
        Some(existing) => {
            if existing.is_write || decl.is_read() {
                return;
            }
            InputDeclaration::new(existing.substate_id.clone(), existing.version, true)
        },
        None => {
            inputs.insert(decl);
            return;
        },
    };
    // `replace` keeps the declaration at its original position, which the signing preimage depends on.
    inputs.replace(widened);
}

/// [`declare_input`] for each of `decls`.
pub fn declare_inputs<I: IntoIterator<Item = InputDeclaration>>(inputs: &mut IndexSet<InputDeclaration>, decls: I) {
    for decl in decls {
        declare_input(inputs, decl);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct InputDeclarationRef<'a> {
    pub substate_id: &'a SubstateId,
    pub version: Option<u32>,
    pub is_write: bool,
}

impl<'a> InputDeclarationRef<'a> {
    pub fn new(substate_id: &'a SubstateId, version: Option<u32>, is_write: bool) -> Self {
        Self {
            substate_id,
            version,
            is_write,
        }
    }

    pub fn read(substate_id: &'a SubstateId) -> Self {
        Self::new(substate_id, None, false)
    }

    pub fn write(substate_id: &'a SubstateId) -> Self {
        Self::new(substate_id, None, true)
    }

    pub fn substate_id(&self) -> &'a SubstateId {
        self.substate_id
    }

    pub fn version(&self) -> Option<u32> {
        self.version
    }

    pub fn is_write(&self) -> bool {
        self.is_write
    }

    pub fn is_read(&self) -> bool {
        !self.is_write
    }

    pub fn lock_type(&self) -> SubstateLockType {
        if self.is_write {
            SubstateLockType::Write
        } else {
            SubstateLockType::Read
        }
    }

    pub fn to_owned(&self) -> InputDeclaration {
        InputDeclaration::new(self.substate_id.clone(), self.version, self.is_write)
    }

    pub fn to_substate_requirement_ref(&self) -> SubstateRequirementRef<'a> {
        SubstateRequirementRef::new(self.substate_id, self.version)
    }

    pub fn with_version(self, version: u32) -> crate::VersionedSubstateIdRef<'a> {
        crate::VersionedSubstateIdRef::new(self.substate_id, version)
    }

    pub fn or_zero_version(self) -> crate::VersionedSubstateIdRef<'a> {
        let v = self.version.unwrap_or(0);
        self.with_version(v)
    }
}

impl<'a> From<&'a InputDeclaration> for InputDeclarationRef<'a> {
    fn from(value: &'a InputDeclaration) -> Self {
        value.as_ref()
    }
}

impl<'a> From<InputDeclarationRef<'a>> for SubstateRequirementRef<'a> {
    fn from(value: InputDeclarationRef<'a>) -> Self {
        value.to_substate_requirement_ref()
    }
}

impl Borrow<SubstateId> for InputDeclarationRef<'_> {
    fn borrow(&self) -> &SubstateId {
        self.substate_id
    }
}

impl AsRef<SubstateId> for InputDeclarationRef<'_> {
    fn as_ref(&self) -> &SubstateId {
        self.substate_id
    }
}

impl PartialEq for InputDeclarationRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.substate_id == other.substate_id
    }
}

impl Eq for InputDeclarationRef<'_> {}

impl std::hash::Hash for InputDeclarationRef<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.substate_id.hash(state);
    }
}

impl Display for InputDeclarationRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.version {
            Some(v) => write!(f, "{}:{}", self.substate_id, v)?,
            None => write!(f, "{}:?", self.substate_id)?,
        }
        if !self.is_write {
            write!(f, ":read")?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to parse input declaration {0}")]
pub struct InputDeclarationParseError(String);

#[cfg(test)]
mod tests {
    use tari_template_lib_types::ComponentAddress;

    use super::*;

    fn component() -> SubstateId {
        SubstateId::Component(ComponentAddress::from_array([1u8; 32]))
    }

    #[test]
    fn an_undeclared_intent_is_a_write() {
        let id: InputDeclaration = component().into();
        assert!(id.is_write());
        assert!(InputDeclaration::from_str(&component().to_string()).unwrap().is_write());
    }

    #[test]
    fn it_parses_version_and_intent_in_either_combination() {
        let id = component();
        let cases = [
            (format!("{id}"), (None, true)),
            (format!("{id}:?"), (None, true)),
            (format!("{id}:7"), (Some(7), true)),
            (format!("{id}:?:read"), (None, false)),
            (format!("{id}:read"), (None, false)),
            (format!("{id}:write"), (None, true)),
            (format!("{id}:7:read"), (Some(7), false)),
            (format!("{id}:7:write"), (Some(7), true)),
        ];
        for (s, (version, is_write)) in cases {
            let decl = InputDeclaration::from_str(&s).unwrap_or_else(|e| panic!("{s}: {e}"));
            assert_eq!(decl.version(), version, "{s}");
            assert_eq!(decl.is_write(), is_write, "{s}");
        }
    }

    #[test]
    fn it_rejects_an_intent_before_a_version() {
        let id = component();
        for s in [
            format!("{id}:read:7"),
            format!("{id}:nonsense"),
            format!("{id}:7:maybe"),
        ] {
            assert!(InputDeclaration::from_str(&s).is_err(), "{s} parsed");
        }
    }

    #[test]
    fn it_round_trips_through_display() {
        let id = component();
        for decl in [
            InputDeclaration::read(id.clone()),
            InputDeclaration::write(id.clone()),
            InputDeclaration::read_versioned(id.clone(), 3),
            InputDeclaration::write_versioned(id, 3),
        ] {
            let parsed = InputDeclaration::from_str(&decl.to_string()).unwrap();
            assert_eq!(parsed.substate_id(), decl.substate_id());
            assert_eq!(parsed.version(), decl.version());
            assert_eq!(parsed.is_write(), decl.is_write());
        }
    }

    #[test]
    fn a_set_holds_one_declaration_per_substate() {
        let mut set = IndexSet::new();
        assert!(set.insert(InputDeclaration::read(component())));
        assert!(!set.insert(InputDeclaration::write(component())));
        assert!(set[0].is_read());
    }

    #[test]
    fn declaring_a_substate_twice_keeps_the_wider_intent() {
        for (first, second) in [
            (
                InputDeclaration::read(component()),
                InputDeclaration::write(component()),
            ),
            (
                InputDeclaration::write(component()),
                InputDeclaration::read(component()),
            ),
        ] {
            let mut set = IndexSet::new();
            declare_input(&mut set, first);
            declare_input(&mut set, second);
            assert_eq!(set.len(), 1);
            assert!(set[0].is_write());
        }
    }

    #[test]
    fn declaring_a_read_twice_stays_a_read() {
        let mut set = IndexSet::new();
        declare_inputs(&mut set, [
            InputDeclaration::read(component()),
            InputDeclaration::read_versioned(component(), 4),
        ]);
        assert_eq!(set.len(), 1);
        assert!(set[0].is_read());
    }
}
