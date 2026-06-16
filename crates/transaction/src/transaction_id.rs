//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt,
    fmt::{Display, Formatter},
};

use borsh::BorshSerialize;
use tari_ootle_common_types::{SubstateAddress, ToSubstateAddress};
use tari_template_lib_types::{
    Hash32,
    KeyParseError,
    TransactionReceiptAddress,
    hex::{fixed_bytes_from_hex, write_hex_fmt},
};

#[derive(
    Copy,
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    BorshSerialize,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionId(
    #[n(0)]
    #[cbor(with = "minicbor::bytes")]
    #[cfg_attr(feature = "serde", serde(with = "ootle_serde::hex"))]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    [u8; 32],
);

impl TransactionId {
    pub const fn new(id: [u8; Self::byte_size()]) -> Self {
        Self(id)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn into_array(self) -> [u8; Self::byte_size()] {
        self.0
    }

    pub fn as_hash(&self) -> Hash32 {
        Hash32::from(self.0)
    }

    pub fn from_hex(hex: &str) -> Result<Self, KeyParseError> {
        let bytes = fixed_bytes_from_hex(hex)?;
        Ok(Self(bytes))
    }

    pub fn from_receipt_address(address: TransactionReceiptAddress) -> Self {
        Self::new(address.as_object_key().into_array())
    }

    pub const fn byte_size() -> usize {
        32
    }

    pub fn into_receipt_address(self) -> TransactionReceiptAddress {
        self.into_array().into()
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|&b| b == 0)
    }
}

impl ToSubstateAddress for TransactionId {
    fn to_substate_address(&self) -> SubstateAddress {
        SubstateAddress::for_transaction_receipt(self.into_receipt_address())
    }
}

impl AsRef<[u8]> for TransactionId {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl AsRef<TransactionId> for TransactionId {
    fn as_ref(&self) -> &TransactionId {
        self
    }
}

impl Display for TransactionId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write_hex_fmt(f, self.as_bytes())
    }
}

impl TryFrom<Vec<u8>> for TransactionId {
    type Error = KeyParseError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Self::try_from(value.as_slice())
    }
}

impl TryFrom<&[u8]> for TransactionId {
    type Error = KeyParseError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() != TransactionId::byte_size() {
            return Err(KeyParseError);
        }
        let mut id = [0u8; TransactionId::byte_size()];
        id.copy_from_slice(value);
        Ok(TransactionId::new(id))
    }
}

impl From<[u8; 32]> for TransactionId {
    fn from(id: [u8; 32]) -> Self {
        Self::new(id)
    }
}

impl From<TransactionId> for Hash32 {
    fn from(id: TransactionId) -> Self {
        Hash32::from(id.0)
    }
}

impl From<Hash32> for TransactionId {
    fn from(hash: Hash32) -> Self {
        Self::new(hash.into_array())
    }
}

impl From<TransactionReceiptAddress> for TransactionId {
    fn from(address: TransactionReceiptAddress) -> Self {
        Self::new(address.as_object_key().into_array())
    }
}
