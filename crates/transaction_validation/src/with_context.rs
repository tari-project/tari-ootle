//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::marker::PhantomData;

use crate::Validator;

/// Marker payload that names `C`, `T` and `E` without owning them, so `WithContext` stays `Send`/`Sync`
/// whatever they are.
type PhantomUnowned<C, T, E> = PhantomData<fn() -> (C, T, E)>;

#[derive(Debug)]
pub struct WithContext<C, T, E>(PhantomUnowned<C, T, E>);

impl<C, T, E> WithContext<C, T, E> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<C, T, E> Validator<T> for WithContext<C, T, E> {
    type Context = C;
    type Error = E;

    fn validate(&self, _context: &C, _input: &T) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<C, T, E> Default for WithContext<C, T, E> {
    fn default() -> Self {
        Self::new()
    }
}
