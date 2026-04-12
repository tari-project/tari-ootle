//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::{Arc, Weak},
};

use tari_ootle_common_types::{
    Network,
    engine_types::substate::{Substate, SubstateId},
};
use tari_ootle_transaction::UnsignedTransaction;

use crate::{
    Address,
    provider::{ProviderError, WantInput},
};

pub type ProviderResult<T> = Result<T, ProviderError>;

/// Core provider trait for interacting with the Ootle network.
///
/// Provides network information, input resolution for transactions, and substate fetching.
/// Implement this trait to create custom providers backed by different transports.
pub trait Provider {
    type Client;

    fn network(&self) -> Network;

    fn weak_client(&self) -> Weak<Self::Client>;

    fn client_upgrade(&mut self) -> Option<Arc<Self::Client>> {
        self.weak_client().upgrade()
    }

    fn default_signer_address(&self) -> &Address;
    fn resolve_input_want_list(
        &self,
        transaction: UnsignedTransaction,
        want_list: &HashSet<WantInput>,
    ) -> impl Future<Output = ProviderResult<UnsignedTransaction>> + Send;

    fn fetch_substates<I: IntoIterator<Item = SubstateId> + Send>(
        &self,
        substate_ids: I,
    ) -> impl Future<Output = ProviderResult<HashMap<SubstateId, Substate>>> + Send;
}

/// Extension of [`Provider`] that includes wallet access for signing and submitting transactions.
pub trait WalletProvider: Provider {
    type Wallet;

    fn wallet(&self) -> &Self::Wallet;

    fn wallet_mut(&mut self) -> &mut Self::Wallet;
}
