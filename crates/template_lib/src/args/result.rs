//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_bor::{BorError, from_value, to_value};
use tari_template_abi::rust::prelude::*;

/// The result of an instruction invocation, which is either the CBOR encoded result value or a `String` with an error
/// message
#[derive(Clone, Debug, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InvokeResult(#[n(0)] Result<tari_bor::Value, String>);

impl InvokeResult {
    pub fn from_value(value: tari_bor::Value) -> Self {
        Self(Ok(value))
    }

    pub fn encode<T: Encode<()> + ?Sized>(output: &T) -> Result<Self, BorError> {
        let value = to_value(output)?;
        Ok(Self(Ok(value)))
    }

    pub fn decode<T: for<'b> Decode<'b, ()>>(self) -> Result<T, BorError> {
        match self.0 {
            Ok(output) => from_value(&output),
            Err(err) => Err(BorError::new(err)),
        }
    }

    pub fn into_value(self) -> Result<tari_bor::Value, BorError> {
        self.0.map_err(BorError::new)
    }

    pub fn unit() -> Self {
        Self(Ok(tari_bor::Value::Array(Vec::new())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_decode() {
        from_value::<()>(&InvokeResult::unit().0.unwrap()).unwrap();
    }
}
