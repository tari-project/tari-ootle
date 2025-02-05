//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

//! This module provides the serialization and deserialization of the SubstateId enum.
//! The implementation is the same as the derive trait implementation, except in the human-readable case where the
//! substate id is (de)serialized as a string.
//! This allows SubstateId to be a key for a map when serializing as JSON (string key required)

use std::{fmt, fmt::Formatter, marker::PhantomData, str::FromStr};

use tari_template_lib::models::{
    ComponentAddress,
    NonFungibleAddress,
    NonFungibleIndexAddress,
    ResourceAddress,
    UnclaimedConfidentialOutputAddress,
    VaultId,
};

use crate::{
    published_template::PublishedTemplateAddress,
    substate::SubstateId,
    transaction_receipt::TransactionReceiptAddress,
    vn_fee_pool::ValidatorFeePoolAddress,
};

impl serde::Serialize for SubstateId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        if serializer.is_human_readable() {
            return serializer.collect_str(&self);
        }

        match *self {
            SubstateId::Component(ref __field0) => {
                serde::Serializer::serialize_newtype_variant(serializer, "SubstateId", 0u32, "Component", __field0)
            },
            SubstateId::Resource(ref __field0) => {
                serde::Serializer::serialize_newtype_variant(serializer, "SubstateId", 1u32, "Resource", __field0)
            },
            SubstateId::Vault(ref __field0) => {
                serde::Serializer::serialize_newtype_variant(serializer, "SubstateId", 2u32, "Vault", __field0)
            },
            SubstateId::UnclaimedConfidentialOutput(ref __field0) => serde::Serializer::serialize_newtype_variant(
                serializer,
                "SubstateId",
                3u32,
                "UnclaimedConfidentialOutput",
                __field0,
            ),
            SubstateId::NonFungible(ref __field0) => {
                serde::Serializer::serialize_newtype_variant(serializer, "SubstateId", 4u32, "NonFungible", __field0)
            },
            SubstateId::NonFungibleIndex(ref __field0) => serde::Serializer::serialize_newtype_variant(
                serializer,
                "SubstateId",
                5u32,
                "NonFungibleIndex",
                __field0,
            ),
            SubstateId::TransactionReceipt(ref __field0) => serde::Serializer::serialize_newtype_variant(
                serializer,
                "SubstateId",
                6u32,
                "TransactionReceipt",
                __field0,
            ),
            SubstateId::Template(ref __field0) => {
                serde::Serializer::serialize_newtype_variant(serializer, "SubstateId", 7u32, "Template", __field0)
            },
            SubstateId::ValidatorFeePool(ref addr) => {
                serde::Serializer::serialize_newtype_variant(serializer, "SubstateId", 8u32, "ValidatorFeePool", addr)
            },
        }
    }
}

impl<'de> serde::Deserialize<'de> for SubstateId {
    #[allow(clippy::too_many_lines)]
    fn deserialize<__D>(deserializer: __D) -> Result<Self, __D::Error>
    where __D: serde::Deserializer<'de> {
        #[allow(non_camel_case_types)]
        #[doc(hidden)]
        enum __Field {
            __field0,
            __field1,
            __field2,
            __field3,
            __field4,
            __field5,
            __field6,
            __field7,
            __field8,
        }
        #[doc(hidden)]
        struct __FieldVisitor;
        impl serde::de::Visitor<'_> for __FieldVisitor {
            type Value = __Field;

            fn expecting(&self, __formatter: &mut Formatter) -> fmt::Result {
                Formatter::write_str(__formatter, "variant identifier")
            }

            fn visit_u64<__E>(self, __value: u64) -> Result<Self::Value, __E>
            where __E: serde::de::Error {
                match __value {
                    0u64 => Ok(__Field::__field0),
                    1u64 => Ok(__Field::__field1),
                    2u64 => Ok(__Field::__field2),
                    3u64 => Ok(__Field::__field3),
                    4u64 => Ok(__Field::__field4),
                    5u64 => Ok(__Field::__field5),
                    6u64 => Ok(__Field::__field6),
                    7u64 => Ok(__Field::__field7),
                    8u64 => Ok(__Field::__field8),
                    _ => Err(serde::de::Error::invalid_value(
                        serde::de::Unexpected::Unsigned(__value),
                        &"variant index 0 <= i < 9",
                    )),
                }
            }

            fn visit_str<__E>(self, __value: &str) -> Result<Self::Value, __E>
            where __E: serde::de::Error {
                match __value {
                    "Component" => Ok(__Field::__field0),
                    "Resource" => Ok(__Field::__field1),
                    "Vault" => Ok(__Field::__field2),
                    "UnclaimedConfidentialOutput" => Ok(__Field::__field3),
                    "NonFungible" => Ok(__Field::__field4),
                    "NonFungibleIndex" => Ok(__Field::__field5),
                    "TransactionReceipt" => Ok(__Field::__field6),
                    "Template" => Ok(__Field::__field7),
                    "ValidatorFeePool" => Ok(__Field::__field8),
                    _ => Err(serde::de::Error::unknown_variant(__value, VARIANTS)),
                }
            }

            fn visit_bytes<__E>(self, __value: &[u8]) -> Result<Self::Value, __E>
            where __E: serde::de::Error {
                match __value {
                    b"Component" => Ok(__Field::__field0),
                    b"Resource" => Ok(__Field::__field1),
                    b"Vault" => Ok(__Field::__field2),
                    b"UnclaimedConfidentialOutput" => Ok(__Field::__field3),
                    b"NonFungible" => Ok(__Field::__field4),
                    b"NonFungibleIndex" => Ok(__Field::__field5),
                    b"TransactionReceipt" => Ok(__Field::__field6),
                    b"Template" => Ok(__Field::__field7),
                    b"ValidatorFeePool" => Ok(__Field::__field8),
                    _ => {
                        let __value = &String::from_utf8_lossy(__value);
                        Err(serde::de::Error::unknown_variant(__value, VARIANTS))
                    },
                }
            }
        }
        impl<'de> serde::Deserialize<'de> for __Field {
            #[inline]
            fn deserialize<__D>(__deserializer: __D) -> Result<Self, __D::Error>
            where __D: serde::Deserializer<'de> {
                serde::Deserializer::deserialize_identifier(__deserializer, __FieldVisitor)
            }
        }
        #[doc(hidden)]
        struct __Visitor<'de> {
            marker: PhantomData<SubstateId>,
            lifetime: PhantomData<&'de ()>,
        }
        impl<'de> serde::de::Visitor<'de> for __Visitor<'de> {
            type Value = SubstateId;

            fn expecting(&self, __formatter: &mut Formatter) -> fmt::Result {
                Formatter::write_str(__formatter, "enum SubstateId")
            }

            fn visit_enum<__A>(self, __data: __A) -> Result<Self::Value, __A::Error>
            where __A: serde::de::EnumAccess<'de> {
                match serde::de::EnumAccess::variant(__data)? {
                    (__Field::__field0, __variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<ComponentAddress>(__variant),
                        SubstateId::Component,
                    ),
                    (__Field::__field1, __variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<ResourceAddress>(__variant),
                        SubstateId::Resource,
                    ),
                    (__Field::__field2, __variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<VaultId>(__variant),
                        SubstateId::Vault,
                    ),
                    (__Field::__field3, __variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<UnclaimedConfidentialOutputAddress>(__variant),
                        SubstateId::UnclaimedConfidentialOutput,
                    ),
                    (__Field::__field4, __variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<NonFungibleAddress>(__variant),
                        SubstateId::NonFungible,
                    ),
                    (__Field::__field5, __variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<NonFungibleIndexAddress>(__variant),
                        SubstateId::NonFungibleIndex,
                    ),
                    (__Field::__field6, __variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<TransactionReceiptAddress>(__variant),
                        SubstateId::TransactionReceipt,
                    ),
                    (__Field::__field7, __variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<PublishedTemplateAddress>(__variant),
                        SubstateId::Template,
                    ),
                    (__Field::__field8, __variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<ValidatorFeePoolAddress>(__variant),
                        SubstateId::ValidatorFeePool,
                    ),
                }
            }
        }
        #[doc(hidden)]
        const VARIANTS: &[&str] = &[
            "Component",
            "Resource",
            "Vault",
            "UnclaimedConfidentialOutput",
            "NonFungible",
            "NonFungibleIndex",
            "TransactionReceipt",
            "Template",
            "ValidatorFeePool",
        ];
        if deserializer.is_human_readable() {
            let s = String::deserialize(deserializer)?;
            SubstateId::from_str(&s).map_err(serde::de::Error::custom)
        } else {
            serde::Deserializer::deserialize_enum(deserializer, "SubstateId", VARIANTS, __Visitor {
                marker: PhantomData::<SubstateId>,
                lifetime: PhantomData,
            })
        }
    }
}
