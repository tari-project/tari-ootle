//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::optional::IsNotFoundError;

use crate::{
    models::WalletEvent,
    storage::{WalletEventStoreWriter, WalletStorageError, WriteableWalletStore},
};

#[derive(Clone)]
pub struct EventsApi<'a, TStore> {
    store: &'a TStore,
}

impl<'a, TStore> EventsApi<'a, TStore> {
    pub(crate) fn new(store: &'a TStore) -> Self {
        Self { store }
    }
}

impl<'a, TStore: WriteableWalletStore> EventsApi<'a, TStore> {
    pub fn log_event(&self, event: &WalletEvent) -> Result<(), EventApiError> {
        self.store.with_write_tx(|tx| tx.append_wallet_event(event))?;
        Ok(())
    }

    // pub fn search_events(
    //     &self,
    //     event_type: Option<WalletEvent>,
    //     limit: usize,
    //     offset: usize,
    // ) -> Result<Vec<WalletEvent>, EventApiError> {
    //     let events = self
    //         .store
    //         .with_read_tx(|tx| tx.search_wallet_events(event_type, limit, offset))?;
    //     Ok(events)
    // }
}

#[derive(thiserror::Error, Debug)]
pub enum EventApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
}

impl IsNotFoundError for EventApiError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, EventApiError::StoreError(e) if e.is_not_found_error())
    }
}
