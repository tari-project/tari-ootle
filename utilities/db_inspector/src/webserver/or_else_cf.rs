//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::Serialize;
use tari_state_store_rocksdb::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
    traits::Cf,
};

pub struct EitherCf<F, G> {
    _phantom: std::marker::PhantomData<(F, G)>,
}

impl<F, G> EitherCf<F, G> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<F, G> Cf for EitherCf<F, G>
where
    F: Cf,
    G: Cf,
{
    type Key = EitherValue<F::Key, G::Key>;
    type KeyCodec = EitherCodec<F::KeyCodec, G::KeyCodec>;
    type Value = EitherValue<F::Value, G::Value>;
    type ValueCodec = EitherCodec<F::ValueCodec, G::ValueCodec>;

    fn name() -> &'static str {
        // F and G must have the same name
        F::name()
    }
}

#[derive(Serialize)]
pub enum EitherValue<V1, V2> {
    First(V1),
    Second(V2),
}

#[derive(Default)]
pub struct EitherCodec<C1, C2> {
    first: C1,
    second: C2,
}

impl<V1, V2, C1, C2> DbCodec<EitherValue<V1, V2>> for EitherCodec<C1, C2>
where
    C1: DbCodec<V1>,
    C2: DbCodec<V2>,
{
    fn encode(&self, value: &EitherValue<V1, V2>) -> Result<EncodeVec, RocksDbStorageError> {
        match value {
            EitherValue::First(v) => self.first.encode(v),
            EitherValue::Second(v) => self.second.encode(v),
        }
    }

    fn decode(&self, bytes: &[u8]) -> Result<EitherValue<V1, V2>, RocksDbStorageError> {
        if let Ok(v) = self.first.decode(bytes) {
            return Ok(EitherValue::First(v));
        }
        self.second.decode(bytes).map(EitherValue::Second)
    }
}
