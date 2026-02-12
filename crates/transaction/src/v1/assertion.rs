//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tari_template_lib_types::{Amount, NonFungibleId, ResourceAddress};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Assertion {
    #[serde(rename = "bkt_amt")]
    BucketAmount {
        resource_address: ResourceAddress,
        is: CheckOrd,
        amount: Amount,
    },
    #[serde(rename = "nt_nil")]
    IsNotNull,
    #[serde(rename = "bkt_ctn_nft")]
    BucketContainsNonFungibles {
        resource_address: ResourceAddress,
        nfts: Box<[NonFungibleId]>,
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
                write!(f, "BucketAtLeast({} {is} {})", resource_address, amount)
            },
            Self::IsNotNull => write!(f, "IsNotNull"),
            Self::BucketContainsNonFungibles {
                resource_address,
                nfts: non_fungible_addresses,
            } => {
                write!(
                    f,
                    "BucketContainsNonFungibles({}, [{}])",
                    resource_address,
                    non_fungible_addresses
                        .iter()
                        .map(|id| id.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
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
