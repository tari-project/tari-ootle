//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub trait ValueLookupTable {
    type Error: std::error::Error;
    fn lookup(&mut self, value: u64) -> Result<Option<[u8; 32]>, Self::Error>;
}

impl<F, E> ValueLookupTable for F
where
    F: FnMut(u64) -> Result<Option<[u8; 32]>, E>,
    E: std::error::Error,
{
    type Error = E;

    fn lookup(&mut self, value: u64) -> Result<Option<[u8; 32]>, Self::Error> {
        (self)(value)
    }
}

pub struct AndThenLookup<T1, T2> {
    first: T1,
    fallback: T2,
}

impl<T1, T2> AndThenLookup<T1, T2> {
    pub fn new(first: T1, fallback: T2) -> Self {
        Self { first, fallback }
    }
}

impl<T1, T2> ValueLookupTable for AndThenLookup<T1, T2>
where
    T1: ValueLookupTable,
    T2: ValueLookupTable<Error = T1::Error>,
{
    type Error = T1::Error;

    fn lookup(&mut self, value: u64) -> Result<Option<[u8; 32]>, Self::Error> {
        if let Some(val) = self.first.lookup(value)? {
            Ok(Some(val))
        } else {
            self.fallback.lookup(value)
        }
    }
}

pub struct MapErrLookup<T, F> {
    inner: T,
    map_err: F,
}

impl<T, F> MapErrLookup<T, F> {
    pub fn new(inner: T, map_err: F) -> Self {
        Self { inner, map_err }
    }
}

impl<T, F, E> ValueLookupTable for MapErrLookup<T, F>
where
    T: ValueLookupTable,
    F: FnMut(T::Error) -> E + 'static,
    E: std::error::Error + 'static,
{
    type Error = E;

    fn lookup(&mut self, value: u64) -> Result<Option<[u8; 32]>, Self::Error> {
        self.inner.lookup(value).map_err(|e| (self.map_err)(e))
    }
}
