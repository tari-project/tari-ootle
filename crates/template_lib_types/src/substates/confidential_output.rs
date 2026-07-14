//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_bor::BorTag;
use tari_template_abi::rust::{
    fmt,
    fmt::{Display, Formatter},
    prelude::*,
    str::FromStr,
};

use super::{BinaryTag, ResourceAddress};
use crate::{KeyParseError, address_prefixes, crypto::PedersenCommitmentBytes};

const TAG: u64 = BinaryTag::ConfidentialOutput.as_u64();

/// Address of a confidential output substate: a resource-namespaced Pedersen commitment.
///
/// The commitment is the identity, giving network-wide uniqueness (via the duplicate-substate guard).
/// The owning vault is not encoded here — membership in a vault's commitment list is what expresses
/// ownership.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode, CborLen)]
#[cbor(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ConfidentialOutputAddress(BorTag<ConfidentialOutputAddressContents, TAG>);

impl ConfidentialOutputAddress {
    pub const fn new(resource_address: ResourceAddress, commitment: PedersenCommitmentBytes) -> Self {
        Self(BorTag::new(ConfidentialOutputAddressContents {
            resource_address,
            commitment,
        }))
    }

    pub fn resource_address(&self) -> &ResourceAddress {
        &self.0.inner().resource_address
    }

    pub fn commitment(&self) -> &PedersenCommitmentBytes {
        &self.0.inner().commitment
    }

    pub fn into_contents(self) -> ConfidentialOutputAddressContents {
        self.0.into_inner()
    }
}

impl FromStr for ConfidentialOutputAddress {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // coutput_{resource}_{commitment}
        let rest = s.strip_prefix("coutput_").unwrap_or(s);
        let (resource, commitment) = rest.split_once('_').ok_or(KeyParseError)?;
        let resource_addr = ResourceAddress::from_hex(resource)?;
        let commitment = PedersenCommitmentBytes::from_hex(commitment)?;
        Ok(Self::new(resource_addr, commitment))
    }
}

impl Display for ConfidentialOutputAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}_{}_{}",
            address_prefixes::CONFIDENTIAL_OUTPUT,
            self.resource_address().as_object_key(),
            self.commitment()
        )
    }
}

impl From<ConfidentialOutputAddressContents> for ConfidentialOutputAddress {
    fn from(contents: ConfidentialOutputAddressContents) -> Self {
        Self(BorTag::new(contents))
    }
}

#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize, borsh::BorshDeserialize))]
pub struct ConfidentialOutputAddressContents {
    #[n(0)]
    pub resource_address: ResourceAddress,
    #[n(1)]
    pub commitment: PedersenCommitmentBytes,
}

#[cfg(feature = "borsh")]
mod borsh_impls {
    use borsh::io;

    use super::*;

    impl borsh::BorshSerialize for ConfidentialOutputAddress {
        fn serialize<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
            ::borsh::BorshSerialize::serialize(self.0.inner(), writer)
        }
    }

    impl borsh::BorshDeserialize for ConfidentialOutputAddress {
        fn deserialize_reader<R: io::Read>(reader: &mut R) -> io::Result<Self> {
            Ok(Self(BorTag::new(borsh::BorshDeserialize::deserialize_reader(reader)?)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_parses_from_strings() {
        let resource_address =
            ResourceAddress::from_hex("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let commitment =
            PedersenCommitmentBytes::from_hex("3210987654321098765432109876543210987654321098765432109876543210")
                .unwrap();
        let address = ConfidentialOutputAddress::new(resource_address, commitment);
        let address_str = address.to_string();
        let parsed = ConfidentialOutputAddress::from_str(&address_str).unwrap();
        assert_eq!(parsed, address);
    }
}
