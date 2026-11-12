//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, marker::PhantomData, str::FromStr};

use serde::{
    de,
    de::{MapAccess, Visitor},
    Deserialize, Deserializer,
};

pub(crate) fn string_or_struct<'de, T, D, TErr>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'de> + FromStr<Err = TErr>,
    D: Deserializer<'de>,
    TErr: fmt::Display,
{
    // This is a Visitor that forwards string types to T's `FromStr` impl and
    // forwards map types to T's `Deserialize` impl. The `PhantomData` is to
    // keep the compiler from complaining about T being an unused generic type
    // parameter. We need T in order to know the Value type for the Visitor
    // impl.
    struct StringOrStruct<T>(PhantomData<fn() -> T>);

    impl<'de, T, TErr> Visitor<'de> for StringOrStruct<T>
    where
        T: Deserialize<'de> + FromStr<Err = TErr>,
        TErr: fmt::Display,
    {
        type Value = T;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("string or map")
        }

        fn visit_str<E>(self, value: &str) -> Result<T, E>
        where E: de::Error {
            FromStr::from_str(value).map_err(E::custom)
        }

        fn visit_map<M>(self, map: M) -> Result<T, M::Error>
        where M: MapAccess<'de> {
            // `MapAccessDeserializer` is a wrapper that turns a `MapAccess`
            // into a `Deserializer`, allowing it to be used as the input to T's
            // `Deserialize` implementation. T then deserializes itself using
            // the entries from the map visitor.
            Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))
        }
    }

    deserializer.deserialize_any(StringOrStruct(PhantomData))
}

pub fn opt_string_or_struct<'de, T, D, TErr>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de> + FromStr<Err = TErr>,
    D: Deserializer<'de>,
    TErr: fmt::Display,
{
    struct OptStringOrStruct<T>(PhantomData<T>);

    impl<'de, T, TErr> Visitor<'de> for OptStringOrStruct<T>
    where
        T: Deserialize<'de> + FromStr<Err = TErr>,
        TErr: fmt::Display,
    {
        type Value = Option<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a null, a string or map")
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where E: de::Error {
            Ok(None)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where E: de::Error {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where D: Deserializer<'de> {
            string_or_struct(deserializer).map(Some)
        }
    }

    deserializer.deserialize_option(OptStringOrStruct(PhantomData))
}

#[cfg(test)]
mod tests {
    use tari_template_lib_types::ComponentAddress;

    use super::*;
    use crate::ComponentAddressOrName;

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct Inner {
        #[serde(default, deserialize_with = "opt_string_or_struct")]
        account: Option<ComponentAddressOrName>,
    }

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct FlattenedEnvelope {
        #[serde(flatten)]
        inner: Inner,
    }

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    #[serde(tag = "method", rename_all = "snake_case")]
    enum TaggedRequest {
        Get {
            #[serde(default, deserialize_with = "opt_string_or_struct")]
            account: Option<ComponentAddressOrName>,
        },
    }

    #[test]
    fn opt_string_or_struct_handles_null_in_direct_flattened_and_tagged_containers() {
        let direct: Inner = serde_json::from_str(r#"{"account": null}"#).unwrap();
        assert_eq!(direct.account, None);

        let flattened: FlattenedEnvelope = serde_json::from_str(r#"{"account": null}"#).unwrap();
        assert_eq!(flattened.inner.account, None);

        let tagged: TaggedRequest = serde_json::from_str(r#"{"method": "get", "account": null}"#).unwrap();
        assert_eq!(tagged, TaggedRequest::Get { account: None });
    }

    #[test]
    fn opt_string_or_struct_parses_string_and_map_in_tagged_container() {
        let by_name: TaggedRequest = serde_json::from_str(r#"{"method": "get", "account": "alice"}"#).unwrap();
        assert_eq!(by_name, TaggedRequest::Get {
            account: Some(ComponentAddressOrName::Name("alice".to_string())),
        });

        let addr = ComponentAddress::from_array([7u8; 32]);
        let by_map_json = serde_json::json!({
            "method": "get",
            "account": { "ComponentAddress": addr }
        });
        let by_map: TaggedRequest = serde_json::from_value(by_map_json).unwrap();
        assert_eq!(by_map, TaggedRequest::Get {
            account: Some(ComponentAddressOrName::ComponentAddress(addr)),
        });
    }
}
