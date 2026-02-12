//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tari_ootle_common_types::displayable::Displayable;
use tari_template_lib_types::{Amount, MaxVec, NonFungibleId, ResourceAddress};

pub type NftAssertVec = MaxVec<32, NonFungibleId>;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Assertion {
    #[serde(rename = "BktAmt")]
    BucketAmount {
        resource_address: ResourceAddress,
        is: CheckOrd,
        amount: Amount,
    },
    #[serde(rename = "NtNil")]
    IsNotNull,
    #[serde(rename = "BktCtnNft")]
    BucketContainsNonFungibles {
        resource_address: ResourceAddress,
        check: NftCheck,
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum CheckOrd {
    Gt,
    Gte,
    Lt,
    Lte,
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum NftCheck {
    AnyOf,
    AllOf,
    NoneOf,
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
