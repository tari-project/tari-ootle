//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::Transaction;

#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, type = "string"))]
pub struct TransactionEnvelope(
    #[n(0)]
    #[cbor(with = "minicbor::bytes")]
    #[cfg_attr(feature = "serde", serde(with = "ootle_serde::base64"))]
    pub Box<[u8]>,
);

impl TransactionEnvelope {
    pub fn from_raw(data: Box<[u8]>) -> Self {
        Self(data)
    }

    pub fn encode(transaction: Transaction) -> Result<Self, tari_bor::BorError> {
        let bytes = tari_bor::encode(&transaction)?;
        Ok(Self::from_raw(bytes.into_boxed_slice()))
    }

    pub fn decode(&self) -> Result<Transaction, tari_bor::BorError> {
        tari_bor::decode::<Transaction>(&self.0)
    }
}
