//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::future::Future;

use tari_ootle_common_types::SubstateRequirement;
use tari_ootle_transaction::UnsignedTransaction;

use crate::{Address, provider::ProviderError};

pub trait UnsignedTransactionBuilder {
    fn default_signer_address(&self) -> &Address;

    fn add_input<S: Into<SubstateRequirement>>(self, substate_id: S) -> Self;

    fn prepare(self) -> impl Future<Output = Result<UnsignedTransaction, ProviderError>>;
}
