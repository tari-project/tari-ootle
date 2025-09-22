//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

//! This module provides the serialization and deserialization of the SubstateId enum.
//! The implementation is the same as the derive trait implementation, except in the human-readable case where the
//! substate id is (de)serialized as a string.
//! This allows SubstateId to be a key for a map when serializing as JSON (string key required)

use std::{borrow::Cow, fmt, fmt::Formatter, marker::PhantomData, str::FromStr};

use tari_template_lib::models::{
    ClaimedOutputTombstoneAddress,
    ComponentAddress,
    NonFungibleAddress,
    ResourceAddress,
    VaultId,
};

use crate::{
    published_template::PublishedTemplateAddress,
    substate::SubstateId,
    transaction_receipt::TransactionReceiptAddress,
    ValidatorFeePoolAddress,
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
            SubstateId::ClaimedOutputTombstone(ref __field0) => serde::Serializer::serialize_newtype_variant(
                serializer,
                "SubstateId",
                3u32,
                "ClaimedOutputTombstone",
                __field0,
            ),
            SubstateId::NonFungible(ref __field0) => {
                serde::Serializer::serialize_newtype_variant(serializer, "SubstateId", 4u32, "NonFungible", __field0)
            },
            SubstateId::TransactionReceipt(ref __field0) => serde::Serializer::serialize_newtype_variant(
                serializer,
                "SubstateId",
                5u32,
                "TransactionReceipt",
                __field0,
            ),
            SubstateId::Template(ref __field0) => {
                serde::Serializer::serialize_newtype_variant(serializer, "SubstateId", 6u32, "Template", __field0)
            },
            SubstateId::ValidatorFeePool(ref addr) => {
                serde::Serializer::serialize_newtype_variant(serializer, "SubstateId", 7u32, "ValidatorFeePool", addr)
            },
            SubstateId::Utxo(ref addr) => {
                serde::Serializer::serialize_newtype_variant(serializer, "SubstateId", 8u32, "Utxo", addr)
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
        enum Field {
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
            type Value = Field;

            fn expecting(&self, __formatter: &mut Formatter) -> fmt::Result {
                Formatter::write_str(__formatter, "variant identifier")
            }

            fn visit_u64<__E>(self, __value: u64) -> Result<Self::Value, __E>
            where __E: serde::de::Error {
                match __value {
                    0u64 => Ok(Field::__field0),
                    1u64 => Ok(Field::__field1),
                    2u64 => Ok(Field::__field2),
                    3u64 => Ok(Field::__field3),
                    4u64 => Ok(Field::__field4),
                    5u64 => Ok(Field::__field5),
                    6u64 => Ok(Field::__field6),
                    7u64 => Ok(Field::__field7),
                    8u64 => Ok(Field::__field8),
                    _ => Err(serde::de::Error::invalid_value(
                        serde::de::Unexpected::Unsigned(__value),
                        &"variant index 0 <= i < 9",
                    )),
                }
            }

            fn visit_str<__E>(self, __value: &str) -> Result<Self::Value, __E>
            where __E: serde::de::Error {
                match __value {
                    "Component" => Ok(Field::__field0),
                    "Resource" => Ok(Field::__field1),
                    "Vault" => Ok(Field::__field2),
                    "ClaimedOutputTombstone" => Ok(Field::__field3),
                    "NonFungible" => Ok(Field::__field4),
                    "TransactionReceipt" => Ok(Field::__field5),
                    "Template" => Ok(Field::__field6),
                    "ValidatorFeePool" => Ok(Field::__field7),
                    "Utxo" => Ok(Field::__field8),
                    _ => Err(serde::de::Error::unknown_variant(__value, VARIANTS)),
                }
            }

            fn visit_bytes<__E>(self, __value: &[u8]) -> Result<Self::Value, __E>
            where __E: serde::de::Error {
                match __value {
                    b"Component" => Ok(Field::__field0),
                    b"Resource" => Ok(Field::__field1),
                    b"Vault" => Ok(Field::__field2),
                    b"ClaimedOutputTombstone" => Ok(Field::__field3),
                    b"NonFungible" => Ok(Field::__field4),
                    b"TransactionReceipt" => Ok(Field::__field5),
                    b"Template" => Ok(Field::__field6),
                    b"ValidatorFeePool" => Ok(Field::__field7),
                    b"Utxo" => Ok(Field::__field8),
                    _ => {
                        let value = String::from_utf8_lossy(__value);
                        Err(serde::de::Error::unknown_variant(&value, VARIANTS))
                    },
                }
            }
        }
        impl<'de> serde::Deserialize<'de> for Field {
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
                    (Field::__field0, variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<ComponentAddress>(variant),
                        SubstateId::Component,
                    ),
                    (Field::__field1, variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<ResourceAddress>(variant),
                        SubstateId::Resource,
                    ),
                    (Field::__field2, variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<VaultId>(variant),
                        SubstateId::Vault,
                    ),
                    (Field::__field3, variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<ClaimedOutputTombstoneAddress>(variant),
                        SubstateId::ClaimedOutputTombstone,
                    ),
                    (Field::__field4, variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<NonFungibleAddress>(variant),
                        SubstateId::NonFungible,
                    ),
                    (Field::__field5, variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<TransactionReceiptAddress>(variant),
                        SubstateId::TransactionReceipt,
                    ),
                    (Field::__field6, variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<PublishedTemplateAddress>(variant),
                        SubstateId::Template,
                    ),
                    (Field::__field7, variant) => Result::map(
                        serde::de::VariantAccess::newtype_variant::<ValidatorFeePoolAddress>(variant),
                        SubstateId::ValidatorFeePool,
                    ),
                    (Field::__field8, variant) => {
                        Result::map(serde::de::VariantAccess::newtype_variant(variant), SubstateId::Utxo)
                    },
                }
            }
        }
        #[doc(hidden)]
        const VARIANTS: &[&str] = &[
            "Component",
            "Resource",
            "Vault",
            "ClaimedOutputTombstone",
            "NonFungible",
            "TransactionReceipt",
            "Template",
            "ValidatorFeePool",
            "Utxo",
        ];
        if deserializer.is_human_readable() {
            let s = Cow::<str>::deserialize(deserializer)?;
            SubstateId::from_str(&s).map_err(serde::de::Error::custom)
        } else {
            serde::Deserializer::deserialize_enum(deserializer, "SubstateId", VARIANTS, __Visitor {
                marker: PhantomData::<SubstateId>,
                lifetime: PhantomData,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use tari_template_lib::{
        models::{NonFungibleId, UtxoAddress, UtxoId},
        types::{Hash, ObjectKey},
    };

    use super::*;

    #[test]
    fn encoding_and_decoding() {
        let component_id = SubstateId::Component(ComponentAddress::new(ObjectKey::from([0; ObjectKey::LENGTH])));
        check(&component_id);
        let resource_id = SubstateId::Resource(ResourceAddress::new(ObjectKey::from([1; ObjectKey::LENGTH])));
        check(&resource_id);
        let vault_id = SubstateId::Vault(VaultId::new(ObjectKey::from([2; ObjectKey::LENGTH])));
        check(&vault_id);
        let unclaimed_output_id = SubstateId::ClaimedOutputTombstone(ClaimedOutputTombstoneAddress::new(
            ObjectKey::from([3; ObjectKey::LENGTH]),
        ));
        check(&unclaimed_output_id);
        let non_fungible_id = SubstateId::NonFungible(NonFungibleAddress::new(
            resource_id.as_resource_address().unwrap(),
            NonFungibleId::from_string("hello"),
        ));
        check(&non_fungible_id);
        let transaction_receipt_id =
            SubstateId::TransactionReceipt(TransactionReceiptAddress::from_array([123; ObjectKey::LENGTH]));
        check(&transaction_receipt_id);
        let template_id =
            SubstateId::Template(PublishedTemplateAddress::from_hash(Hash::from_array([6; Hash::LENGTH])));
        check(&template_id);
        let validator_fee_pool_id = SubstateId::ValidatorFeePool(ValidatorFeePoolAddress::from_array([7; 32]));
        check(&validator_fee_pool_id);
        let utxo_id = SubstateId::Utxo(UtxoAddress::new(
            resource_id.as_resource_address().unwrap(),
            UtxoId::from_array([8; UtxoId::LENGTH]),
        ));
        check(&utxo_id);

        fn check(id: &SubstateId) {
            // JSON
            let serialized = serde_json::to_string(&id).unwrap();
            assert_eq!(serialized, format!(r#""{}""#, id));

            let deserialized: SubstateId = serde_json::from_str(&serialized).unwrap();
            assert_eq!(deserialized, *id);

            // CBOR
            let cbor_serialized = tari_bor::encode(&id).unwrap();
            let cbor_deserialized: SubstateId = tari_bor::decode(&cbor_serialized)
                .unwrap_or_else(|e| panic!("Failed to deserialize {id} from CBOR: {e}"));
            assert_eq!(cbor_deserialized, *id);

            // bincode
            let bincode_serialized = bincode::serde::encode_to_vec(id, bincode::config::standard()).unwrap();
            let (bincode_deserialized, _): (SubstateId, _) =
                bincode::serde::decode_from_slice(&bincode_serialized, bincode::config::standard()).unwrap();
            assert_eq!(bincode_deserialized, *id);
        }
    }
}
