//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::{Read, Write};

use tari_ootle_common_types::ShardGroup;

use crate::{
    codecs::{DbDecoder, DbEncoder, EncodeVec, ShardCodec},
    error::RocksDbStorageError,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct ShardGroupCodec {
    inner: ShardCodec,
}

impl DbEncoder<ShardGroup> for ShardGroupCodec {
    fn encode_len(&self, value: &ShardGroup) -> Result<usize, RocksDbStorageError> {
        let start_len = self.inner.encode_len(&value.start())?;
        let end_len = self.inner.encode_len(&value.end())?;
        Ok(start_len + end_len)
    }

    fn encode_into<W: Write>(&self, value: &ShardGroup, writer: &mut W) -> Result<(), RocksDbStorageError> {
        self.inner.encode_into(&value.start(), writer)?;
        self.inner.encode_into(&value.end(), writer)?;
        Ok(())
    }

    fn encode(&self, value: &ShardGroup) -> Result<EncodeVec, RocksDbStorageError> {
        let start = self.inner.encode(&value.start())?;
        let end = self.inner.encode(&value.end())?;
        Ok(EncodeVec::from_slices(&[start.as_slice(), end.as_slice()]))
    }
}

impl DbDecoder<ShardGroup> for ShardGroupCodec {
    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<ShardGroup, RocksDbStorageError> {
        let start = self.inner.decode_reader(reader)?;
        let end = self.inner.decode_reader(reader)?;
        Ok(ShardGroup::new(start, end))
    }
}
