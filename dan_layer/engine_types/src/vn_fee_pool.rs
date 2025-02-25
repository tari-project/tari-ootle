// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{
    borrow::Cow,
    fmt,
    fmt::{Display, Formatter},
    str::FromStr,
};

use tari_bor::{BorTag, Deserialize, Serialize};
use tari_template_lib::{
    auth::{OwnerRule, Ownership},
    constants::XTR,
    crypto::RistrettoPublicKeyBytes,
    models::{Amount, BinaryTag, KeyParseError, ObjectKey},
    Hash,
};

use crate::resource_container::{ResourceContainer, ResourceError};

const TAG: u64 = BinaryTag::ValidatorNodeFeePool.as_u64();

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct ValidatorFeePoolAddress(#[cfg_attr(feature = "ts", ts(type = "string"))] BorTag<ObjectKey, TAG>);

impl ValidatorFeePoolAddress {
    pub const fn from_array(arr: [u8; ObjectKey::LENGTH]) -> Self {
        let key = ObjectKey::from_array(arr);
        Self(BorTag::new(key))
    }

    pub fn as_object_key(&self) -> &ObjectKey {
        self.0.inner()
    }

    pub fn as_slice(&self) -> &[u8] {
        self.0.inner()
    }

    pub fn from_hex(hex: &str) -> Result<Self, KeyParseError> {
        Ok(Self(BorTag::new(ObjectKey::from_hex(hex)?)))
    }

    pub fn as_hash(&self) -> Hash {
        Hash::from_array(self.as_object_key().into_array())
    }
}

impl From<[u8; 32]> for ValidatorFeePoolAddress {
    fn from(arr: [u8; 32]) -> Self {
        Self::from_array(arr)
    }
}

impl Display for ValidatorFeePoolAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "vnfp_{}", self.as_object_key())
    }
}

impl TryFrom<&[u8]> for ValidatorFeePoolAddress {
    type Error = KeyParseError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() != ObjectKey::LENGTH {
            return Err(KeyParseError);
        }

        let mut key = [0u8; ObjectKey::LENGTH];
        key.copy_from_slice(value);
        Ok(Self::from_array(key))
    }
}

impl FromStr for ValidatorFeePoolAddress {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix("vnfp_").unwrap_or(s);
        Self::from_hex(s)
    }
}

impl borsh::BorshSerialize for ValidatorFeePoolAddress {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        borsh::BorshSerialize::serialize(self.as_object_key().array(), writer)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct ValidatorFeePool {
    #[cfg_attr(feature = "ts", ts(type = "ArrayBuffer"))]
    pub claim_public_key: RistrettoPublicKeyBytes,
    pub amount: Amount,
}

impl ValidatorFeePool {
    pub fn new(claim_public_key: RistrettoPublicKeyBytes, amount: Amount) -> Self {
        Self {
            claim_public_key,
            amount,
        }
    }

    pub fn as_ownership(&self) -> Ownership<'_> {
        Ownership {
            owner_key: Some(&self.claim_public_key),
            owner_rule: Cow::Owned(OwnerRule::OwnedBySigner),
        }
    }

    pub fn deposit(&mut self, amount: Amount) -> &mut Self {
        self.amount += amount;
        self
    }

    pub fn withdraw_all(&mut self) -> Result<ResourceContainer, ResourceError> {
        if self.amount.is_zero() {
            return Err(ResourceError::InsufficientBalance {
                details: "ValidatorFeePool has insufficient balance. Current balance is 0".to_string(),
            });
        }
        let amount = self.amount;
        self.amount = Amount::zero();
        Ok(ResourceContainer::Confidential {
            address: XTR,
            commitments: Default::default(),
            revealed_amount: amount,
            locked_commitments: Default::default(),
            locked_revealed_amount: Default::default(),
        })
    }
}
