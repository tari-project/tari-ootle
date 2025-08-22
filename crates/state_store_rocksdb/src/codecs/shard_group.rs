//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::Read;

use tari_ootle_common_types::ShardGroup;

use crate::{
    codecs::{DbCodec, EncodeVec, ShardCodec},
    error::RocksDbStorageError,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct ShardGroupCodec {
    inner: ShardCodec,
}

impl DbCodec<ShardGroup> for ShardGroupCodec {
    fn encode(&self, value: &ShardGroup) -> Result<EncodeVec, RocksDbStorageError> {
        let start = self.inner.encode(&value.start())?;
        let end = self.inner.encode(&value.end())?;
        Ok(EncodeVec::from_slices(&[start.as_slice(), end.as_slice()]))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<ShardGroup, RocksDbStorageError> {
        let start = self.inner.decode_reader(reader)?;
        let end = self.inner.decode_reader(reader)?;
        Ok(ShardGroup::new(start, end))
    }
}
