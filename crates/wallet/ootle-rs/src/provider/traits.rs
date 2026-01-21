//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, future::Future, sync::Weak};

use tari_ootle_transaction::UnsignedTransaction;

use crate::{
    provider::{ProviderError, WantInput},
    Address,
};

pub type ProviderResult<T> = Result<T, ProviderError>;
pub trait Provider {
    type Client;

    fn network(&self) -> tari_ootle_wallet_sdk::Network;

    fn weak_client(&self) -> Weak<Self::Client>;
    fn default_signer_address(&self) -> &Address;
    fn resolve_input_want_list(
        &self,
        transaction: UnsignedTransaction,
        want_list: &HashSet<WantInput>,
    ) -> impl Future<Output = ProviderResult<UnsignedTransaction>> + Send;
}
