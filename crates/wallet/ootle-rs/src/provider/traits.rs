//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::{Arc, Weak},
};

use tari_ootle_common_types::{
    engine_types::substate::{Substate, SubstateId},
    Network,
};
use tari_ootle_transaction::UnsignedTransaction;

use crate::{
    provider::{ProviderError, WantInput},
    Address,
};

pub type ProviderResult<T> = Result<T, ProviderError>;
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

pub trait WalletProvider: Provider {
    type Wallet;

    fn wallet(&self) -> &Self::Wallet;

    fn wallet_mut(&mut self) -> &mut Self::Wallet;
}
