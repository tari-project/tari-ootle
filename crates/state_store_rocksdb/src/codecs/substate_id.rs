//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::{Cursor, Write};

use borsh::BorshDeserialize;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{SubstateVersion, VersionedSubstateIdRef};

use crate::{
    codecs::{DbDecoder, DbEncoder, EncodeVec},
    error::RocksDbStorageError,
};

#[derive(Clone, Copy, Default)]
pub struct SubstateIdCodec;

impl SubstateIdCodec {
    fn get_substate_id_encoded_len(self, value: &SubstateId) -> Result<usize, RocksDbStorageError> {
        let len = borsh::object_length(value).map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })?;
        Ok(len)
    }

    fn encode_substate_id_into<W: Write>(self, value: &SubstateId, writer: &mut W) -> Result<(), RocksDbStorageError> {
        borsh::to_writer(writer, value).map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })?;
        Ok(())
    }
}

impl DbEncoder<SubstateId> for SubstateIdCodec {
    fn encode_len(&self, value: &SubstateId) -> Result<usize, RocksDbStorageError> {
        self.get_substate_id_encoded_len(value)
    }

    fn encode_into<W: Write>(&self, value: &SubstateId, writer: &mut W) -> Result<(), RocksDbStorageError> {
        self.encode_substate_id_into(value, writer)
    }
}

impl DbDecoder<SubstateId> for SubstateIdCodec {
    fn decode(&self, bytes: &[u8]) -> Result<(SubstateId, usize), RocksDbStorageError> {
        // We use deserialize_reader (rather than from_slice) so that no error is returned if there
        // are extra bytes in the buffer — this codec is used as a prefix in composite keys.
        // The cursor tracks how many bytes borsh consumed.
        let mut cursor = Cursor::new(bytes);
        let value = SubstateId::deserialize_reader(&mut cursor)
            .map_err(|e| RocksDbStorageError::DecodeError { source: e.into() })?;
        Ok((value, cursor.position() as usize))
    }
}

impl<'a> DbEncoder<&'a SubstateId> for SubstateIdCodec {
    fn encode_len(&self, value: &&'a SubstateId) -> Result<usize, RocksDbStorageError> {
        self.get_substate_id_encoded_len(value)
    }

    fn encode_into<W: Write>(&self, value: &&'a SubstateId, writer: &mut W) -> Result<(), RocksDbStorageError> {
        self.encode_substate_id_into(value, writer)
    }

    fn encode(&self, value: &&'a SubstateId) -> Result<EncodeVec, RocksDbStorageError> {
        SubstateIdCodec.encode(*value)
    }
}

impl<'a> DbEncoder<VersionedSubstateIdRef<'a>> for SubstateIdCodec {
    fn encode_len(&self, value: &VersionedSubstateIdRef<'a>) -> Result<usize, RocksDbStorageError> {
        self.get_substate_id_encoded_len(value.substate_id())
            .map(|len| len + size_of::<SubstateVersion>())
    }

    fn encode_into<W: Write>(
        &self,
        value: &VersionedSubstateIdRef<'a>,
        writer: &mut W,
    ) -> Result<(), RocksDbStorageError> {
        self.encode_substate_id_into(value.substate_id(), writer)?;
        writer
            .write_all(&value.version().to_be_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow::anyhow!("VersionedSubstateIdRefCodec: Failed to write version: {}", e),
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tari_template_lib_types::{ComponentAddress, NonFungibleAddress, NonFungibleId, ObjectKey, ResourceAddress};

    use super::*;

    fn new_substate_id(seed: u8) -> SubstateId {
        let component = ComponentAddress::from_array([seed; ObjectKey::LENGTH]);
        SubstateId::Component(component)
    }

    #[test]
    fn encode_decode() {
        let substate_id = new_substate_id(1);
        let codec = SubstateIdCodec;
        let encoded = codec.encode(&substate_id).unwrap();
        let decoded: SubstateId = codec.decode_exact(&encoded).unwrap();
        assert_eq!(substate_id, decoded);
    }

    #[test]
    fn encode_versioned_substate_id_ref_preserves_full_u64_version() {
        let substate_id = new_substate_id(1);
        let codec = SubstateIdCodec;
        let id_bytes = codec.encode(&substate_id).unwrap();

        let v1 = VersionedSubstateIdRef::new(&substate_id, SubstateVersion::new(1));
        let v2 = VersionedSubstateIdRef::new(&substate_id, SubstateVersion::new(2));

        let encoded_v1 = codec.encode(&v1).unwrap();
        let encoded_v2 = codec.encode(&v2).unwrap();

        assert_eq!(encoded_v1.len(), codec.encode_len(&v1).unwrap());
        assert_eq!(encoded_v1.len(), id_bytes.len() + size_of::<SubstateVersion>());
        assert_eq!(&encoded_v1[..id_bytes.len()], id_bytes.as_slice());
        assert_eq!(&encoded_v1[id_bytes.len()..], &1u64.to_be_bytes());
        assert_ne!(encoded_v1, encoded_v2);

        let (decoded_id, consumed) = codec.decode(&encoded_v1).unwrap();
        assert_eq!(decoded_id, substate_id);
        assert_eq!(consumed, id_bytes.len());

        // A SubstateId whose borsh encoding is 95 bytes produces a 103-byte versioned key (spilling past
        // EncodeVec::FIXED_SIZE = 100); under-reporting encode_len by 4 bytes previously took the 100-byte stack path
        // and failed with WriteZero.
        let nft_id = NonFungibleId::try_from_string("a".repeat(41)).unwrap();
        let long_id = SubstateId::NonFungible(NonFungibleAddress::new(
            ResourceAddress::from_array([2; ObjectKey::LENGTH]),
            nft_id,
        ));
        let long_id_len = codec.encode_len(&long_id).unwrap();
        assert!((93..=96).contains(&long_id_len), "unexpected long_id_len: {long_id_len}");
        let versioned_long = VersionedSubstateIdRef::new(&long_id, SubstateVersion::new(0x0102_0304_0506_0708));
        let encoded_long = codec.encode(&versioned_long).unwrap();
        assert_eq!(encoded_long.len(), long_id_len + size_of::<SubstateVersion>());
        assert_eq!(&encoded_long[long_id_len..], &0x0102_0304_0506_0708u64.to_be_bytes());
    }
}
