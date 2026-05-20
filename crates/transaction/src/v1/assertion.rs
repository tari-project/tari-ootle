//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tari_ootle_common_types::displayable::Displayable;
use tari_template_lib_types::{Amount, MaxVec, NonFungibleId, ResourceAddress};

pub type NftAssertVec = MaxVec<32, NonFungibleId>;

#[derive(
    Debug,
    Clone,
    Deserialize,
    Serialize,
    PartialEq,
    borsh::BorshSerialize,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Assertion {
    #[n(0)]
    #[serde(rename = "BktAmt")]
    BucketAmount {
        #[n(0)]
        resource_address: ResourceAddress,
        #[n(1)]
        is: CheckOrd,
        #[n(2)]
        amount: Amount,
    },
    #[n(1)]
    #[serde(rename = "NtNil")]
    IsNotNull,
    #[n(2)]
    #[serde(rename = "BktCtnNft")]
    BucketContainsNonFungibles {
        #[n(0)]
        resource_address: ResourceAddress,
        #[n(1)]
        check: NftCheck,
        #[n(2)]
        #[cfg_attr(feature = "ts", ts(as = "Vec<NonFungibleId>"))]
        nfts: NftAssertVec,
    },
}

impl Display for Assertion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BucketAmount {
                resource_address,
                is,
                amount,
            } => {
                write!(f, "BucketAmount({} {is} {})", resource_address, amount)
            },
            Self::IsNotNull => write!(f, "IsNotNull"),
            Self::BucketContainsNonFungibles {
                resource_address,
                check,
                nfts: non_fungible_addresses,
            } => {
                write!(
                    f,
                    "BucketContainsNonFungibles({} {} [{}])",
                    resource_address,
                    check,
                    non_fungible_addresses.display()
                )
            },
        }
    }
}

#[derive(
    Debug,
    Clone,
    Deserialize,
    Serialize,
    PartialEq,
    borsh::BorshSerialize,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum CheckOrd {
    #[n(0)]
    Gt,
    #[n(1)]
    Gte,
    #[n(2)]
    Lt,
    #[n(3)]
    Lte,
    #[n(4)]
    Eq,
}

impl CheckOrd {
    pub fn check<T: PartialEq<U> + Eq + PartialOrd<U> + Ord, U>(&self, left: T, right: U) -> bool {
        match self {
            CheckOrd::Gt => left > right,
            CheckOrd::Gte => left >= right,
            CheckOrd::Lt => left < right,
            CheckOrd::Lte => left <= right,
            CheckOrd::Eq => left == right,
        }
    }
}

impl Display for CheckOrd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Gt => ">",
            Self::Gte => ">=",
            Self::Lt => "<",
            Self::Lte => "<=",
            Self::Eq => "==",
        };
        write!(f, "{s}")
    }
}

#[derive(
    Debug,
    Clone,
    Deserialize,
    Serialize,
    PartialEq,
    borsh::BorshSerialize,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum NftCheck {
    #[n(0)]
    AnyOf,
    #[n(1)]
    AllOf,
    #[n(2)]
    NoneOf,
    #[n(3)]
    NotAllOf,
}

impl Display for NftCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::AnyOf => "any of",
            Self::AllOf => "all of",
            Self::NoneOf => "none of",
            Self::NotAllOf => "not all of",
        };
        write!(f, "{s}")
    }
}
