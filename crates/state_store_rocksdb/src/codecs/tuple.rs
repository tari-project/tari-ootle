//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::Read;

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
};

impl<A, B, C, CA, CB, CC> DbCodec<(A, B, C)> for (CA, CB, CC)
where
    CA: DbCodec<A>,
    CB: DbCodec<B>,
    CC: DbCodec<C>,
{
    fn encode(&self, (a, b, c): &(A, B, C)) -> Result<EncodeVec, RocksDbStorageError> {
        let (ca, cb, cc) = &self;
        let a_bytes = ca.encode(a)?;
        let b_bytes = cb.encode(b)?;
        let c_bytes = cc.encode(c)?;
        Ok(EncodeVec::from_slices(&[
            a_bytes.as_ref(),
            b_bytes.as_ref(),
            c_bytes.as_ref(),
        ]))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<(A, B, C), RocksDbStorageError> {
        let (ca, cb, cc) = &self;
        let a = ca.decode_reader(reader)?;
        let b = cb.decode_reader(reader)?;
        let c = cc.decode_reader(reader)?;
        Ok((a, b, c))
    }
}

impl<A, B, CA, CB> DbCodec<(A, B)> for (CA, CB)
where
    CA: DbCodec<A>,
    CB: DbCodec<B>,
{
    fn encode(&self, (a, b): &(A, B)) -> Result<EncodeVec, RocksDbStorageError> {
        let (ca, cb) = &self;
        let a_bytes = ca.encode(a)?;
        let b_bytes = cb.encode(b)?;
        Ok(EncodeVec::from_slices(&[a_bytes.as_ref(), b_bytes.as_ref()]))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<(A, B), RocksDbStorageError> {
        let (ca, cb) = &self;
        let a = ca.decode_reader(reader)?;
        let b = cb.decode_reader(reader)?;
        Ok((a, b))
    }
}
