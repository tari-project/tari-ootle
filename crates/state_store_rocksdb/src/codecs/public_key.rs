//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::Read;

use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    codecs::{DbCodec, EncodeVec, FixedBytesCodec},
    error::RocksDbStorageError,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct PublicKeyCodec {
    inner: FixedBytesCodec<{ RistrettoPublicKeyBytes::length() }>,
}

impl DbCodec<RistrettoPublicKeyBytes> for PublicKeyCodec {
    fn encode(&self, value: &RistrettoPublicKeyBytes) -> Result<EncodeVec, RocksDbStorageError> {
        self.inner.encode(value)
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<RistrettoPublicKeyBytes, RocksDbStorageError> {
        self.inner.decode_reader(reader)
    }
}
