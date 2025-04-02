//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use tari_dan_common_types::VersionedSubstateId;
use tari_engine_types::substate::SubstateId;
use tari_transaction::TransactionId;

use crate::{
    codecs::{DbCodec, EncodeVec, SubstateIdCodec},
    error::RocksDbStorageError,
    model::substate::SubstateTransactionIndexKey,
    utils::read_to_fixed,
};

#[derive(Default)]
pub struct SubstateTransactionKeyCodec {
    substate_id_codec: SubstateIdCodec,
}

impl SubstateTransactionKeyCodec {
    fn decode_transaction_id(&self, reader: &mut &[u8]) -> Result<TransactionId, RocksDbStorageError> {
        let buf = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!(
                "SubstateTransactionIndexCodec: Invalid bytes len={} for FixedHash",
                reader.len()
            ),
        })?;
        Ok(TransactionId::new(buf))
    }

    fn decode_bool(&self, reader: &mut &[u8]) -> Result<bool, RocksDbStorageError> {
        if reader.is_empty() {
            return Err(RocksDbStorageError::MalformedData {
                operation: "decode bool",
                details: "Empty bytes".to_string(),
            });
        }
        let b = reader[0] != 0;
        *reader = &reader[1..];
        Ok(b)
    }

    fn decode_u32(&self, reader: &mut &[u8]) -> Result<u32, RocksDbStorageError> {
        let bytes = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!(
                "SubstateTransactionIndexCodec: Invalid bytes len={} for u32",
                reader.len()
            ),
        })?;
        Ok(u32::from_be_bytes(bytes))
    }

    fn decode_substate_id(&self, reader: &mut &[u8]) -> Result<SubstateId, RocksDbStorageError> {
        let substate_id = self.substate_id_codec.decode_reader(reader)?;
        Ok(substate_id)
    }
}

impl DbCodec<SubstateTransactionIndexKey> for SubstateTransactionKeyCodec {
    fn encode(&self, value: &SubstateTransactionIndexKey) -> Result<EncodeVec, RocksDbStorageError> {
        let tx_id_bytes = value.transaction_id.as_bytes();
        let substate_id_bytes = self
            .substate_id_codec
            .encode(value.versioned_substate_id.substate_id())?;
        let version_bytes = value.versioned_substate_id.version().to_be_bytes();
        let bool_bytes = [u8::from(value.is_up)];
        Ok(EncodeVec::from_slices(&[
            tx_id_bytes,
            &substate_id_bytes,
            &version_bytes,
            &bool_bytes,
        ]))
    }

    fn decode(&self, mut bytes: &[u8]) -> Result<SubstateTransactionIndexKey, RocksDbStorageError> {
        let reader = &mut bytes;
        let transaction_id = self.decode_transaction_id(reader)?;
        let substate_id = self.decode_substate_id(reader)?;
        let version = self.decode_u32(reader)?;
        let versioned_substate_id = VersionedSubstateId::new(substate_id, version);
        let is_up = self.decode_bool(reader)?;
        Ok(SubstateTransactionIndexKey {
            is_up,
            versioned_substate_id,
            transaction_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use tari_engine_types::template_models::{ComponentAddress, ObjectKey};

    use super::*;

    fn create_substate_id(seed: u8) -> SubstateId {
        SubstateId::Component(ComponentAddress::from_array([seed; ObjectKey::LENGTH]))
    }

    #[test]
    fn encode_decode() {
        let codec = SubstateTransactionKeyCodec::default();
        let key = SubstateTransactionIndexKey {
            is_up: true,
            versioned_substate_id: VersionedSubstateId::new(create_substate_id(1), 1),
            transaction_id: TransactionId::default(),
        };
        let encoded = codec.encode(&key).unwrap();
        let decoded = codec.decode(&encoded).unwrap();
        assert_eq!(key.versioned_substate_id, decoded.versioned_substate_id);
        assert_eq!(key.transaction_id, decoded.transaction_id);
        assert_eq!(key.is_up, decoded.is_up);
    }
}
