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
        tari_bor::decode_exact::<Transaction>(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use tari_ootle_common_types::{Epoch, crypto::create_key_pair};

    use super::*;

    #[test]
    fn encode_and_decode_round_trips_and_rejects_trailing_bytes() {
        let (secret, _) = create_key_pair();
        let tx = Transaction::builder_localnet(Epoch(1))
            .drop_all_proofs_in_workspace()
            .build_and_seal(&secret);
        let expected_id = tx.calculate_id();

        let envelope = TransactionEnvelope::encode(tx).unwrap();
        let decoded = envelope.decode().unwrap();
        assert_eq!(decoded.calculate_id(), expected_id);

        let mut trailing = envelope.0.into_vec();
        trailing.push(0x00);
        let malleated = TransactionEnvelope::from_raw(trailing.into_boxed_slice());
        assert!(malleated.decode().is_err());
    }
}
