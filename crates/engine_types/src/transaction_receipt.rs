//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt,
    fmt::{Display, Formatter},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use tari_bor::BorTag;
use tari_common_types::types::FixedHash;
use tari_template_lib::{
    models::{address_prefixes, BinaryTag},
    types::{Hash, KeyParseError, ObjectKey},
};

use crate::{
    events::Event,
    fees::FeeReceipt,
    logs::LogEntry,
    serde_with,
    substate::{hash_substate, SubstateDiff, SubstateId},
    ValidatorFeeWithdrawal,
};

const TAG: u64 = BinaryTag::TransactionReceipt.as_u64();

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
pub struct TransactionReceiptAddress(#[cfg_attr(feature = "ts", ts(type = "string"))] BorTag<ObjectKey, TAG>);

impl TransactionReceiptAddress {
    pub const fn from_hash(hash: Hash) -> Self {
        Self::from_array(hash.into_array())
    }

    pub const fn from_array(arr: [u8; ObjectKey::LENGTH]) -> Self {
        let key = ObjectKey::from_array(arr);
        Self(BorTag::new(key))
    }

    pub const fn as_object_key(&self) -> &ObjectKey {
        self.0.inner()
    }

    pub fn from_hex(hex: &str) -> Result<Self, KeyParseError> {
        Ok(Self(BorTag::new(ObjectKey::from_hex(hex)?)))
    }
}

impl<T: Into<Hash>> From<T> for TransactionReceiptAddress {
    fn from(address: T) -> Self {
        Self::from_hash(address.into())
    }
}

impl Display for TransactionReceiptAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}_{}", address_prefixes::TRANSACTION_RECEIPT, self.as_object_key())
    }
}

impl FromStr for TransactionReceiptAddress {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix("txreceipt_").unwrap_or(s);
        Self::from_hex(s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionReceipt {
    pub diff_summary: DiffSummary,
    pub fee_withdrawals: Box<[ValidatorFeeWithdrawal]>,
    pub events: Box<[Event]>,
    pub logs: Box<[LogEntry]>,
    pub fee_receipt: FeeReceipt,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DiffSummary {
    pub upped: Box<[UpSubstate]>,
}

impl From<&SubstateDiff> for DiffSummary {
    fn from(diff: &SubstateDiff) -> Self {
        Self {
            upped: diff
                .up_iter()
                .map(|(id, s)| UpSubstate {
                    substate_id: id.clone(),
                    version: s.version(),
                    value_hash: hash_substate(s.substate_value(), s.version()),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpSubstate {
    pub substate_id: SubstateId,
    pub version: u32,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[serde(with = "serde_with::hex")]
    pub value_hash: FixedHash,
}
