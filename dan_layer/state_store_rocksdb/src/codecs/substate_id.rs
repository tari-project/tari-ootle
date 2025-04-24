//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io;

use borsh::BorshDeserialize;
use tari_engine_types::substate::SubstateId;

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
};

#[derive(Clone, Copy, Default)]
pub struct SubstateIdCodec;

impl SubstateIdCodec {
    fn encode_substate_id(self, len: usize, value: &SubstateId) -> Result<EncodeVec, RocksDbStorageError> {
        if len < EncodeVec::FIXED_SIZE {
            let mut buf = EncodeVec::make_stack_buf();
            borsh::to_writer(buf.as_mut_slice(), value)
                .map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })?;
            return Ok(EncodeVec::new_stack(buf, len));
        }
        let buf = borsh::to_vec(value).map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })?;
        Ok(EncodeVec::new_heap(buf))
    }

    pub fn decode_reader<R: io::Read>(self, reader: &mut R) -> Result<SubstateId, RocksDbStorageError> {
        // We use the deserialize_reader here so that no error is returned if there are extra bytes in the buffer
        // in order to use SubstateId as a prefix
        SubstateId::deserialize_reader(reader).map_err(|e| RocksDbStorageError::DecodeError { source: e.into() })
    }
}

impl DbCodec<SubstateId> for SubstateIdCodec {
    fn encode(&self, value: &SubstateId) -> Result<EncodeVec, RocksDbStorageError> {
        let len = borsh::object_length(value).map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })?;
        self.encode_substate_id(len, value)
    }

    fn decode(&self, mut bytes: &[u8]) -> Result<SubstateId, RocksDbStorageError> {
        self.decode_reader(&mut bytes)
    }
}

impl<'a> DbCodec<&'a SubstateId> for SubstateIdCodec {
    fn encode(&self, value: &&'a SubstateId) -> Result<EncodeVec, RocksDbStorageError> {
        SubstateIdCodec.encode(*value)
    }

    fn decode(&self, _bytes: &[u8]) -> Result<&'a SubstateId, RocksDbStorageError> {
        panic!("Attempt to decode a reference to a SubstateId which is not possible.")
    }
}

#[cfg(test)]
mod tests {
    use tari_common_types::types::FixedHash;
    use tari_dan_storage::consensus_models::BlockId;
    use tari_engine_types::template_lib_models::ComponentAddress;
    use tari_template_lib_types::ObjectKey;

    use super::*;
    use crate::{codecs::BlockDiffKeyCodec, models::block_diff::BlockDiffKey};

    fn new_substate_id(seed: u8) -> SubstateId {
        let component = ComponentAddress::from_array([seed; ObjectKey::LENGTH]);
        SubstateId::Component(component)
    }

    fn new_block_id(seed: u8) -> BlockId {
        BlockId::new(FixedHash::from([seed; FixedHash::byte_size()]))
    }

    #[test]
    fn encode_decode() {
        let substate_id = new_substate_id(1);
        let codec = SubstateIdCodec;
        let encoded = codec.encode(&substate_id).unwrap();
        let decoded: SubstateId = codec.decode(&encoded).unwrap();
        assert_eq!(substate_id, decoded);
    }

    #[test]
    fn check_with_prefix() {
        // This codec happens to encode {len}||{substate_id} as a prefix
        let compound_codec = BlockDiffKeyCodec::default();
        let key = BlockDiffKey {
            block_id: new_block_id(1),
            substate_id: new_substate_id(2),
            version: 3,
        };

        let compound_encoding = compound_codec.encode(&key).unwrap();
        let codec = SubstateIdCodec;
        let encoded = codec.encode(&key.substate_id).unwrap();
        assert_eq!(&compound_encoding.as_slice()[..encoded.len()], encoded.as_slice());
        let decoded = compound_codec.decode(&compound_encoding).unwrap();
        assert_eq!(key.substate_id, decoded.substate_id);
    }
}
