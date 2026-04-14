//   Copyright 2022 The Tari Project
//   SPDX-License-Identifier: BSD-3-clause

use std::str::FromStr;

use syn::{Lit, parse2};
use tari_bor::{BorError, Serialize};
use tari_engine_types::substate::SubstateId;
use tari_ootle_transaction::{args::InstructionArg, call_arg};
use tari_template_lib_types::{NonFungibleId, hex::bytes_from_hex};

use crate::error::ManifestError;

#[derive(Debug, Clone)]
pub enum ManifestValue {
    SubstateId(SubstateId),
    Literal(Lit),
    NonFungibleId(NonFungibleId),
    Value(tari_bor::Value),
}

impl ManifestValue {
    pub fn new_value<T: Serialize>(value: &T) -> Result<Self, BorError> {
        Ok(Self::Value(tari_bor::to_value(value)?))
    }

    pub fn as_address(&self) -> Option<&SubstateId> {
        match self {
            Self::SubstateId(addr) => Some(addr),
            _ => None,
        }
    }

    pub fn to_arg(&self) -> Result<InstructionArg, ManifestError> {
        match self {
            ManifestValue::SubstateId(addr) => match addr {
                SubstateId::Component(addr) => Ok(call_arg!(*addr)),
                SubstateId::Resource(addr) => Ok(call_arg!(*addr)),
                // TODO: should tx receipt addresses be allowed to be referenced?
                SubstateId::TransactionReceipt(addr) => Ok(call_arg!(*addr)),
                SubstateId::Vault(addr) => Ok(call_arg!(*addr)),
                SubstateId::NonFungible(addr) => Ok(call_arg!(addr)),
                SubstateId::ClaimedOutputTombstone(addr) => Ok(call_arg!(*addr)),
                SubstateId::Template(addr) => Ok(call_arg!(*addr)),
                SubstateId::ValidatorFeePool(addr) => Ok(call_arg!(*addr)),
                SubstateId::Utxo(addr) => Ok(call_arg!(*addr)),
            },
            ManifestValue::Literal(lit) => lit_to_arg(lit),
            ManifestValue::NonFungibleId(id) => Ok(call_arg!(id.clone())),
            ManifestValue::Value(blob) => Ok(InstructionArg::literal(blob.clone()).unwrap()),
        }
    }
}

impl<T: Into<SubstateId>> From<T> for ManifestValue {
    fn from(addr: T) -> Self {
        ManifestValue::SubstateId(addr.into())
    }
}

pub fn lit_to_arg(lit: &Lit) -> Result<InstructionArg, ManifestError> {
    match lit {
        Lit::Str(s) => Ok(call_arg!(s.value())),
        Lit::Int(i) => match i.suffix() {
            "u8" => Ok(call_arg!(i.base10_parse::<u8>()?)),
            "u16" => Ok(call_arg!(i.base10_parse::<u16>()?)),
            "u32" => Ok(call_arg!(i.base10_parse::<u32>()?)),
            "u64" => Ok(call_arg!(i.base10_parse::<u64>()?)),
            "u128" => Ok(call_arg!(i.base10_parse::<u128>()?)),
            "i8" => Ok(call_arg!(i.base10_parse::<i8>()?)),
            "i16" => Ok(call_arg!(i.base10_parse::<i16>()?)),
            "i32" => Ok(call_arg!(i.base10_parse::<i32>()?)),
            "i64" => Ok(call_arg!(i.base10_parse::<i64>()?)),
            "" | "i128" => Ok(call_arg!(i.base10_parse::<i128>()?)),
            _ => Err(ManifestError::UnsupportedExpr(format!(
                r#"Unsupported integer suffix "{}""#,
                i.suffix()
            ))),
        },
        Lit::Bool(b) => Ok(call_arg!(b.value())),
        Lit::ByteStr(v) => Ok(call_arg!(v.value())),
        Lit::Byte(v) => Ok(call_arg!(v.value())),
        Lit::Char(v) => Ok(call_arg!(v.value().to_string())),
        Lit::Float(v) => Err(ManifestError::UnsupportedExpr(format!(
            "Float literals not supported ({})",
            v
        ))),
        Lit::Verbatim(v) => Err(ManifestError::UnsupportedExpr(format!(
            "Raw token literals not supported ({})",
            v
        ))),
        _ => Err(ManifestError::UnsupportedExpr(format!(
            "Unsupported literal type ({:?})",
            lit
        ))),
    }
}

// https://github.com/rust-lang/rfcs/issues/2758 :/
// impl From<NonFungibleId> for ManifestValue {
//     fn from(id: NonFungibleId) -> Self {
//         ManifestValue::NonFungibleId(id)
//     }
// }

impl FromStr for ManifestValue {
    type Err = ManifestParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        SubstateId::from_str(s)
            .ok()
            .map(ManifestValue::SubstateId)
            .or_else(|| {
                let id = NonFungibleId::try_from_canonical_string(s).ok()?;
                Some(ManifestValue::NonFungibleId(id))
            })
            .or_else(|| {
                let tokens = s.parse().ok()?;
                let lit: Lit = parse2(tokens).ok()?;
                // Reject Lit::Int with unrecognized suffixes (e.g. hex strings like "044bccd4..."
                // that syn misinterprets as integer + suffix)
                if let Lit::Int(ref i) = lit {
                    match i.suffix() {
                        "" | "u8" | "u16" | "u32" | "u64" | "u128" | "i8" | "i16" | "i32" | "i64" | "i128" => {},
                        _ => return None,
                    }
                }
                Some(ManifestValue::Literal(lit))
            })
            .or_else(|| {
                // Try parsing as hex bytes (e.g. public keys)
                let bytes = bytes_from_hex(s).ok()?;
                Some(ManifestValue::Value(tari_bor::Value::Bytes(bytes)))
            })
            .ok_or_else(|| ManifestParseError(s.to_string()))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid manifest value '{0}'")]
pub struct ManifestParseError(String);

#[cfg(test)]
mod tests {
    use tari_template_lib_types::{ComponentAddress, ResourceAddress, VaultId};

    use super::*;

    #[test]
    fn it_parses_hex_bytes() {
        let val = "044bccd4d01ceb41816bc9106a836806e6f9412646ecda4c2d726d8372b2c843"
            .parse::<ManifestValue>()
            .unwrap();
        assert!(matches!(val, ManifestValue::Value(tari_bor::Value::Bytes(_))));
    }

    #[test]
    fn it_parses_address_strings() {
        let addr = "component_0000000000000000000000000000000000000000000000000000000000000000"
            .parse::<ManifestValue>()
            .unwrap();
        assert_eq!(
            *addr.as_address().unwrap(),
            SubstateId::Component(
                ComponentAddress::from_hex("0000000000000000000000000000000000000000000000000000000000000000").unwrap()
            )
        );

        let addr = "resource_0000000000000000000000000000000000000000000000000000000000000000"
            .parse::<ManifestValue>()
            .unwrap();
        assert_eq!(
            *addr.as_address().unwrap(),
            SubstateId::Resource(
                ResourceAddress::from_hex("0000000000000000000000000000000000000000000000000000000000000000").unwrap()
            )
        );

        let addr = "vault_0000000000000000000000000000000000000000000000000000000000000000"
            .parse::<ManifestValue>()
            .unwrap();
        assert_eq!(
            *addr.as_address().unwrap(),
            SubstateId::Vault(
                VaultId::from_hex("0000000000000000000000000000000000000000000000000000000000000000").unwrap()
            )
        );
    }
}
