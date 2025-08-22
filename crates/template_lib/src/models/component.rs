//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

use tari_bor::BorTag;
use tari_template_lib_types::{EntityId, KeyParseError, ObjectKey};

use crate::{models::BinaryTag, newtype_struct_serde_impl};

const TAG: u64 = BinaryTag::ComponentAddress.as_u64();

/// A component's unique global identifier.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ComponentAddress(#[cfg_attr(feature = "ts", ts(type = "string"))] BorTag<ObjectKey, TAG>);

impl ComponentAddress {
    /// Creates a new `ComponentAddress` from an `ObjectKey`. For internal use only.
    pub const fn new(substate_key: ObjectKey) -> Self {
        Self(BorTag::new(substate_key))
    }

    /// Returns the underlying `ObjectKey` of this `ComponentAddress`.
    pub fn as_object_key(&self) -> &ObjectKey {
        &self.0
    }

    /// Returns the underlying byte slice.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }

    /// Creates a new `ComponentAddress` from a hex string.
    pub fn from_hex(hex: &str) -> Result<Self, KeyParseError> {
        let key = ObjectKey::from_hex(hex)?;
        Ok(Self::new(key))
    }

    /// Creates a new `ComponentAddress` from a byte array.
    pub fn from_array(arr: [u8; ObjectKey::LENGTH]) -> Self {
        Self::new(ObjectKey::from_array(arr))
    }

    /// Returns the `EntityId` associated with this component address.
    pub fn entity_id(&self) -> EntityId {
        self.0.inner().as_entity_id()
    }
}

impl FromStr for ComponentAddress {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix("component_").unwrap_or(s);
        Self::from_hex(s)
    }
}

impl<T: Into<ObjectKey>> From<T> for ComponentAddress {
    fn from(address: T) -> Self {
        Self::new(address.into())
    }
}

impl Display for ComponentAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "component_{}", *self.0)
    }
}

impl AsRef<[u8]> for ComponentAddress {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

newtype_struct_serde_impl!(ComponentAddress, BorTag<ObjectKey, TAG>);

#[cfg(feature = "borsh")]
mod borsh {
    use std::io::Read;

    use super::*;

    impl ::borsh::BorshSerialize for ComponentAddress {
        fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
            self.as_object_key().array().serialize(writer)
        }
    }

    impl ::borsh::BorshDeserialize for ComponentAddress {
        fn deserialize_reader<R: Read>(reader: &mut R) -> std::io::Result<Self> {
            let key = ::borsh::BorshDeserialize::deserialize_reader(reader)?;
            Ok(Self::new(ObjectKey::from_array(key)))
        }
    }
}
