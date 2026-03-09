//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use borsh::BorshDeserialize;
use ledger_transport::APDUAnswer;

use crate::LedgerClientError;

pub trait DecodeAnswer<Out> {
    fn decode<E>(&self) -> Result<Out, LedgerClientError<E>>
    where Self: Sized;
}

impl<T: BorshDeserialize, B: Deref<Target = [u8]>> DecodeAnswer<T> for APDUAnswer<B> {
    fn decode<E>(&self) -> Result<T, LedgerClientError<E>>
    where Self: Sized {
        let data = self.data();
        T::try_from_slice(data).map_err(|e| LedgerClientError::InvalidResponse { details: e.to_string() })
    }
}
