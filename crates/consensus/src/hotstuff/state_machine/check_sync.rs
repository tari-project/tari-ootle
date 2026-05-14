//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{marker::PhantomData, time::Duration};

use log::warn;
use tokio::time;

use crate::{
    hotstuff::{
        HotStuffError,
        state_machine::{event::ConsensusStateEvent, idle::Idle, running::Running, worker::ConsensusWorkerContext},
    },
    traits::{ConsensusSpec, SyncManager, SyncStatus},
};

const LOG_TARGET: &str = "tari::ootle::consensus::sm::check_sync";

const INCONCLUSIVE_BACKOFF_INITIAL: Duration = Duration::from_secs(2);
const INCONCLUSIVE_BACKOFF_MAX: Duration = Duration::from_secs(30);
const INCONCLUSIVE_BACKOFF_STEP: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct CheckSync<TSpec>(PhantomData<TSpec>);

impl<TSpec> CheckSync<TSpec>
where
    TSpec: ConsensusSpec,
    HotStuffError: From<<TSpec::SyncManager as SyncManager>::Error>,
{
    pub(super) async fn on_enter(
        &self,
        context: &mut ConsensusWorkerContext<TSpec>,
    ) -> Result<ConsensusStateEvent, HotStuffError> {
        // On an `Inconclusive` probe result we don't want to bounce through the broader state
        // machine (Sleeping → Idle → CheckSync), because the proper recovery is to keep asking
        // peers until quorum responds. Loop locally with a linear backoff.
        let mut backoff = INCONCLUSIVE_BACKOFF_INITIAL;
        loop {
            match context.state_sync.check_sync().await? {
                SyncStatus::UpToDate => return Ok(ConsensusStateEvent::Ready),
                SyncStatus::Behind { target_epoch } => return Ok(ConsensusStateEvent::NeedSync { target_epoch }),
                SyncStatus::Inconclusive => {
                    warn!(
                        target: LOG_TARGET,
                        "🛜 check_sync was inconclusive (peers did not reach quorum); retrying in {:?}",
                        backoff,
                    );
                    time::sleep(backoff).await;
                    backoff = (backoff + INCONCLUSIVE_BACKOFF_STEP).min(INCONCLUSIVE_BACKOFF_MAX);
                },
            }
        }
    }
}

impl<TSpec> From<Idle<TSpec>> for CheckSync<TSpec> {
    fn from(_: Idle<TSpec>) -> Self {
        Self(PhantomData)
    }
}

impl<TSpec> From<Running<TSpec>> for CheckSync<TSpec> {
    fn from(_: Running<TSpec>) -> Self {
        Self(PhantomData)
    }
}
