//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt,
    fmt::{Display, Formatter},
    str::FromStr,
};

use borsh::{BorshDeserialize, BorshSerialize};
use tari_bor::{BorTag, Deserialize, Serialize};
use tari_template_lib::{
    models::{BinaryTag, ResourceAddress},
    prelude::{from_hex, serde_helpers, KeyParseError},
    types::hex::write_hex_fmt,
};

use crate::confidential::ConfidentialOutput;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct Utxo {
    pub output: ConfidentialOutput,
    pub is_frozen: bool,
}

impl Utxo {
    pub fn new(output: ConfidentialOutput) -> Self {
        Self {
            output,
            is_frozen: false,
        }
    }

    pub fn output(&self) -> &ConfidentialOutput {
        &self.output
    }
}

const TAG: u64 = BinaryTag::Utxo.as_u64();

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct UtxoAddress(#[cfg_attr(feature = "ts", ts(type = "string"))] BorTag<UtxoAddressContents, TAG>);

impl UtxoAddress {
    pub fn new(resource_address: ResourceAddress, id: UtxoId) -> Self {
        Self(BorTag::new(UtxoAddressContents { resource_address, id }))
    }

    pub fn resource_address(&self) -> &ResourceAddress {
        &self.0.inner().resource_address
    }

    pub fn id(&self) -> &UtxoId {
        &self.0.inner().id
    }
}

impl FromStr for UtxoAddress {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // utxo_{resource}_{id}
        let rest = s.strip_prefix("utxo_").unwrap_or(s);
        let (resource, id) = rest.split_once('_').ok_or(KeyParseError)?;
        let resource_addr = ResourceAddress::from_hex(resource)?;
        let id = UtxoId::from_hex(id)?;
        Ok(Self::new(resource_addr, id))
    }
}

impl Display for UtxoAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "utxo_{}_{}", self.resource_address().as_object_key(), self.id())
    }
}

impl From<UtxoAddressContents> for UtxoAddress {
    fn from(contents: UtxoAddressContents) -> Self {
        Self(BorTag::new(contents))
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct UtxoId(
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[serde(with = "serde_helpers::fixed_hex")]
    [u8; Self::LENGTH],
);

impl UtxoId {
    pub const LENGTH: usize = 32;

    pub const fn from_array(arr: [u8; 32]) -> Self {
        Self(arr)
    }

    pub fn from_hex(hex: &str) -> Result<Self, KeyParseError> {
        from_hex(hex).map(Self::from_array)
    }
}

impl Display for UtxoId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write_hex_fmt(f, &self.0)
    }
}

/// A NonFungibleId namespaced by a ResourceAddress.
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct UtxoAddressContents {
    resource_address: ResourceAddress,
    id: UtxoId,
}

impl borsh::BorshSerialize for UtxoAddress {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        ::borsh::BorshSerialize::serialize(self.0.inner(), writer)
    }
}

impl borsh::BorshDeserialize for UtxoAddress {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        Ok(Self(BorTag::new(borsh::BorshDeserialize::deserialize_reader(reader)?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_parses_from_strings() {
        let resource_address =
            ResourceAddress::from_hex("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let id = UtxoId::from_hex("3210987654321098765432109876543210987654321098765432109876543210").unwrap();
        let utxo_address = UtxoAddress::new(resource_address, id);
        let utxo_address_str = utxo_address.to_string();
        assert_eq!(
            utxo_address_str,
            "utxo_1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef_3210987654321098765432109876543210987654321098765432109876543210"
        );
        let parsed_utxo_address = UtxoAddress::from_str(&utxo_address_str).unwrap();
        assert_eq!(parsed_utxo_address, utxo_address);
    }
}
