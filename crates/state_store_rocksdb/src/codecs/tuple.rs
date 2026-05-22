//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::Write;

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
    fn decode(&self, bytes: &[u8]) -> Result<((A, B, C), usize), RocksDbStorageError> {
        let (ca, cb, cc) = &self;
        let (a, n_a) = ca.decode(bytes)?;
        let (b, n_b) = cb.decode(&bytes[n_a..])?;
        let (c, n_c) = cc.decode(&bytes[n_a + n_b..])?;
        Ok(((a, b, c), n_a + n_b + n_c))
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
    fn decode(&self, bytes: &[u8]) -> Result<((A, B), usize), RocksDbStorageError> {
        let (ca, cb) = &self;
        let (a, n_a) = ca.decode(bytes)?;
        let (b, n_b) = cb.decode(&bytes[n_a..])?;
        Ok(((a, b), n_a + n_b))
    }
}
