//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_bor::BorError;

#[derive(
    Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, Serialize, Deserialize, borsh::BorshSerialize,
)]
#[cbor(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct NonFungibleContainer(#[n(0)] Option<NonFungible>);

impl NonFungibleContainer {
    pub fn no_data() -> Self {
        Self::new(tari_bor::Value::Null, tari_bor::Value::Null)
    }

    pub fn new(data: tari_bor::Value, mutable_data: tari_bor::Value) -> Self {
        Self(Some(NonFungible::new(data, mutable_data)))
    }

    pub fn contents_mut(&mut self) -> Option<&mut NonFungible> {
        self.0.as_mut()
    }

    pub fn contents(&self) -> Option<&NonFungible> {
        self.0.as_ref()
    }

    pub fn is_burnt(&self) -> bool {
        self.0.is_none()
    }

    pub fn burn(&mut self) {
        self.0 = None;
    }
}

#[derive(
    Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, Serialize, Deserialize, borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct NonFungible {
    #[n(0)]
    #[cfg_attr(feature = "ts", ts(type = "any"))]
    #[serde(with = "ootle_serde::cbor_value")]
    #[borsh(serialize_with = "crate::borsh::serialize_cbor_value")]
    data: tari_bor::Value,
    #[n(1)]
    #[cfg_attr(feature = "ts", ts(type = "any"))]
    #[serde(with = "ootle_serde::cbor_value")]
    #[borsh(serialize_with = "crate::borsh::serialize_cbor_value")]
    mutable_data: tari_bor::Value,
}

impl NonFungible {
    pub fn new(data: tari_bor::Value, mutable_data: tari_bor::Value) -> Self {
        Self { data, mutable_data }
    }

    pub fn data(&self) -> &tari_bor::Value {
        &self.data
    }

    pub fn mutable_data(&self) -> &tari_bor::Value {
        &self.mutable_data
    }

    pub fn decode_mutable_data<T>(&self) -> Result<T, BorError>
    where T: for<'b> tari_bor::Decode<'b, ()> {
        tari_bor::from_value(&self.mutable_data)
    }

    pub fn decode_data<T>(&self) -> Result<T, BorError>
    where T: for<'b> tari_bor::Decode<'b, ()> {
        tari_bor::from_value(&self.data)
    }

    pub fn set_mutable_data(&mut self, mutable_data: tari_bor::Value) {
        self.mutable_data = mutable_data;
    }
}
