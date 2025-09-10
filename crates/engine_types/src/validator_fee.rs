//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    borrow::Cow,
    fmt,
    fmt::{Display, Formatter},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use tari_bor::BorTag;
use tari_template_lib::{
    auth::{OwnerRule, Ownership},
    constants::XTR,
    models::{address_prefixes, BinaryTag},
    types::{crypto::RistrettoPublicKeyBytes, Amount, Hash, KeyParseError, ObjectKey},
};

use crate::resource_container::{ResourceContainer, ResourceError};

const TAG: u64 = BinaryTag::ValidatorNodeFeePool.as_u64();

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
        write!(f, "{}_{}", address_prefixes::VALIDATOR_FEE_POOL, self.as_object_key())
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

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ValidatorFeePool {
    #[cfg_attr(feature = "ts", ts(type = "ArrayBuffer"))]
    pub claim_public_key: RistrettoPublicKeyBytes,
    pub amount: u64,
}

impl ValidatorFeePool {
    pub fn new(claim_public_key: RistrettoPublicKeyBytes, amount: u64) -> Self {
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

    /// Withdraws the given amount from the pool. If the amount is greater than the current balance, the function will
    /// return false and the balance will remain unchanged.
    /// NB: Do not use this function in the engine. This is used at the consensus level to update fee substates in
    /// place.
    #[must_use]
    pub fn withdraw_direct(&mut self, amount: u64) -> bool {
        match self.amount.checked_sub(amount) {
            Some(new_amount) => {
                self.amount = new_amount;
                true
            },
            None => false,
        }
    }

    /// Deposits the given amount into the pool. Will return false and
    /// the balance will remain unchanged if the deposit overflows u64.
    /// NB: Do not use this function in the engine. This is used at the consensus level to update fee substates in
    /// place.
    #[must_use]
    pub fn deposit_direct(&mut self, amount: u64) -> bool {
        match self.amount.checked_add(amount) {
            Some(new_amount) => {
                self.amount = new_amount;
                true
            },
            None => false,
        }
    }

    pub fn amount(&self) -> u64 {
        self.amount
    }

    pub fn claim_public_key(&self) -> &RistrettoPublicKeyBytes {
        &self.claim_public_key
    }

    /// Withdraws all the funds from the pool and returns them in a ResourceContainer.
    /// If the pool has insufficient funds, an error is returned.
    /// This function is used in the engine to withdraw the funds from the pool and create a Bucket.
    pub fn withdraw_all(&mut self) -> Result<ResourceContainer, ResourceError> {
        if self.amount == 0 {
            return Err(ResourceError::InsufficientBalance {
                details: "ValidatorFeePool has insufficient balance. Current balance is 0".to_string(),
            });
        }
        let amount = self.amount;
        self.amount = 0;
        Ok(ResourceContainer::Stealth {
            address: XTR,
            revealed_amount: amount.into(),
            locked_amount: Amount::zero(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ValidatorFeeWithdrawal {
    pub address: ValidatorFeePoolAddress,
    pub amount: u64,
}
