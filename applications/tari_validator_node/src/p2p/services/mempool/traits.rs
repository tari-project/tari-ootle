//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, future::Future};

use indexmap::IndexMap;
use tari_dan_common_types::SubstateRequirement;
use tari_engine_types::substate::{Substate, SubstateId};
use tari_transaction::Transaction;

pub struct ResolvedSubstates {
    pub local: IndexMap<SubstateId, Substate>,
    pub unresolved_foreign: HashSet<SubstateRequirement>,
}

pub trait SubstateResolver {
    type Error: Send + Sync + 'static;

    fn try_resolve_local(&self, transaction: &Transaction) -> Result<ResolvedSubstates, Self::Error>;

    fn try_resolve_foreign(
        &self,
        requested_substates: &HashSet<SubstateRequirement>,
    ) -> impl Future<Output = Result<IndexMap<SubstateId, Substate>, Self::Error>> + Send;
}
