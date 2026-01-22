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

use tari_bor::BorTag;
use tari_template_abi::rust::{
    fmt,
    fmt::{Display, Formatter},
    str::FromStr,
};

use super::BinaryTag;
use crate::{address_prefixes, newtype_struct_serde_impl, EntityId, KeyParseError, ObjectKey};

const TAG: u64 = BinaryTag::VaultId as u64;

/// A vault's unique identification in the Tari network
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize, borsh::BorshDeserialize))]
pub struct VaultId(#[cfg_attr(feature = "ts", ts(type = "string"))] BorTag<ObjectKey, TAG>);

impl VaultId {
    pub const fn new(key: ObjectKey) -> Self {
        Self(BorTag::new(key))
    }

    pub fn from_hex(hex: &str) -> Result<Self, KeyParseError> {
        let key = ObjectKey::from_hex(hex)?;
        Ok(Self::new(key))
    }

    pub const fn as_object_key(&self) -> &ObjectKey {
        self.0.inner()
    }

    pub fn entity_id(&self) -> EntityId {
        self.0.inner().as_entity_id()
    }
}

impl From<ObjectKey> for VaultId {
    fn from(key: ObjectKey) -> Self {
        Self::new(key)
    }
}

impl Display for VaultId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}_{}", address_prefixes::VAULT, *self.0)
    }
}

impl AsRef<[u8]> for VaultId {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl FromStr for VaultId {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix("vault_").unwrap_or(s);
        Self::from_hex(s)
    }
}

impl TryFrom<&[u8]> for VaultId {
    type Error = KeyParseError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let key = ObjectKey::try_from(value)?;
        Ok(Self::new(key))
    }
}

newtype_struct_serde_impl!(VaultId, BorTag<ObjectKey, TAG>);
