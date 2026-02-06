//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::{Read, Write};

use crate::{
    codecs::{DbDecoder, DbEncoder},
    error::RocksDbStorageError,
};

impl<A, B, C, CA, CB, CC> DbEncoder<(A, B, C)> for (CA, CB, CC)
where
    CA: DbEncoder<A>,
    CB: DbEncoder<B>,
    CC: DbEncoder<C>,
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
}

impl<A, B, C, CA, CB, CC> DbDecoder<(A, B, C)> for (CA, CB, CC)
where
    CA: DbDecoder<A>,
    CB: DbDecoder<B>,
    CC: DbDecoder<C>,
{
    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<(A, B, C), RocksDbStorageError> {
        let (ca, cb, cc) = &self;
        let a = ca.decode_reader(reader)?;
        let b = cb.decode_reader(reader)?;
        let c = cc.decode_reader(reader)?;
        Ok((a, b, c))
    }
}

impl<A, B, CA, CB> DbEncoder<(A, B)> for (CA, CB)
where
    CA: DbEncoder<A>,
    CB: DbEncoder<B>,
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
}

impl<A, B, CA, CB> DbDecoder<(A, B)> for (CA, CB)
where
    CA: DbDecoder<A>,
    CB: DbDecoder<B>,
{
    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<(A, B), RocksDbStorageError> {
        let (ca, cb) = &self;
        let a = ca.decode_reader(reader)?;
        let b = cb.decode_reader(reader)?;
        Ok((a, b))
    }
}
