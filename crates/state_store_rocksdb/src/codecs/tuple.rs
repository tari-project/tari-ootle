//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::{Read, Write};

use crate::{codecs::DbCodec, error::RocksDbStorageError};

impl<A, B, C, CA, CB, CC> DbCodec<(A, B, C)> for (CA, CB, CC)
where
    CA: DbCodec<A>,
    CB: DbCodec<B>,
    CC: DbCodec<C>,
{
    fn encode_len(&self, value: &(A, B, C)) -> Result<usize, RocksDbStorageError> {
        let (ca, cb, cc) = &self;
        let a_len = ca.encode_len(&value.0)?;
        let b_len = cb.encode_len(&value.1)?;
        let c_len = cc.encode_len(&value.2)?;
        Ok(a_len + b_len + c_len)
    }

    fn encode_into<W: Write>(&self, (a, b, c): &(A, B, C), writer: &mut W) -> Result<(), RocksDbStorageError> {
        let (ca, cb, cc) = &self;
        ca.encode_into(a, writer)?;
        cb.encode_into(b, writer)?;
        cc.encode_into(c, writer)?;
        Ok(())
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
    fn encode_len(&self, value: &(A, B)) -> Result<usize, RocksDbStorageError> {
        let (ca, cb) = &self;
        let a_len = ca.encode_len(&value.0)?;
        let b_len = cb.encode_len(&value.1)?;
        Ok(a_len + b_len)
    }

    fn encode_into<W: Write>(&self, (a, b): &(A, B), writer: &mut W) -> Result<(), RocksDbStorageError> {
        let (ca, cb) = &self;
        ca.encode_into(a, writer)?;
        cb.encode_into(b, writer)?;
        Ok(())
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<(A, B), RocksDbStorageError> {
        let (ca, cb) = &self;
        let a = ca.decode_reader(reader)?;
        let b = cb.decode_reader(reader)?;
        Ok((a, b))
    }
}
