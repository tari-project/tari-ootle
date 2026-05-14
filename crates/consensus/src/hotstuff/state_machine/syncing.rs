//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::marker::PhantomData;

use tari_ootle_common_types::Epoch;

use crate::{
    hotstuff::{ConsensusWorkerContext, HotStuffError, state_machine::event::ConsensusStateEvent},
    traits::{ConsensusSpec, SyncManager},
};

#[derive(Debug)]
pub struct Syncing<TSpec> {
    /// The epoch the caller proved is the highest finalised one — populated by the `NeedSync`
    /// event that triggered the transition into this state. `None` means use the default sync
    /// target (oracle's current epoch).
    target_epoch: Option<Epoch>,
    _spec: PhantomData<TSpec>,
}

impl<TSpec> Syncing<TSpec>
where
    TSpec: ConsensusSpec,
    HotStuffError: From<<TSpec::SyncManager as SyncManager>::Error>,
{
    pub(super) fn new(target_epoch: Option<Epoch>) -> Self {
        Self {
            target_epoch,
            _spec: PhantomData,
        }
    }

    pub(super) async fn on_enter(
        &self,
        context: &mut ConsensusWorkerContext<TSpec>,
    ) -> Result<ConsensusStateEvent, HotStuffError> {
        context.state_sync.sync(self.target_epoch).await?;
        Ok(ConsensusStateEvent::SyncComplete)
    }
}
