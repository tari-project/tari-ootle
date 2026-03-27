//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::{Read, Write};

use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    codecs::{DbDecoder, DbEncoder, EncodeVec, FixedBytesCodec},
    error::RocksDbStorageError,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct PublicKeyCodec {
    inner: FixedBytesCodec<{ RistrettoPublicKeyBytes::length() }>,
}

impl DbEncoder<RistrettoPublicKeyBytes> for PublicKeyCodec {
    fn encode_len(&self, value: &RistrettoPublicKeyBytes) -> Result<usize, RocksDbStorageError> {
        self.inner.encode_len(value)
    }

    fn encode_into<W: Write>(
        &self,
        value: &RistrettoPublicKeyBytes,
        writer: &mut W,
    ) -> Result<(), RocksDbStorageError> {
        self.inner.encode_into(value, writer)
    }

    fn encode(&self, value: &RistrettoPublicKeyBytes) -> Result<EncodeVec, RocksDbStorageError> {
        self.inner.encode(value)
    }
}

impl DbDecoder<RistrettoPublicKeyBytes> for PublicKeyCodec {
    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<RistrettoPublicKeyBytes, RocksDbStorageError> {
        self.inner.decode_reader(reader)
    }
}
