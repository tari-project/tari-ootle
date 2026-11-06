//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{marker::PhantomData, time::Duration};

use log::*;
use tari_epoch_manager::EpochManagerReader;
use tari_ootle_common_types::{Epoch, ShardGroup};
use tokio::time;

use crate::{
    hotstuff::{ConsensusWorkerContext, HotStuffError, state_machine::event::ConsensusStateEvent},
    traits::{ConsensusSpec, PresyncStatus, SyncManager},
};

const LOG_TARGET: &str = "tari::ootle::consensus::sm::syncing";

/// How long a presync that found no checkpoint waits before `Idle` retries it.
const PRESYNC_RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// Sync the state of the committee this node is a member of in the current epoch, then join consensus.
    Member {
        /// The epoch the caller proved is the highest finalised one, from the `NeedSync` event that triggered the
        /// transition into this state. `None` means use the default sync target (oracle's current epoch).
        target_epoch: Option<Epoch>,
    },
    /// Sync the state of `shard_group` ahead of this node's activation into its committee, then return to `Idle`.
    Presync { shard_group: ShardGroup },
}

#[derive(Debug)]
pub struct Syncing<TSpec> {
    mode: SyncMode,
    _spec: PhantomData<TSpec>,
}

impl<TSpec> Syncing<TSpec> {
    pub(super) fn is_presync(&self) -> bool {
        matches!(self.mode, SyncMode::Presync { .. })
    }
}

impl<TSpec> Syncing<TSpec>
where
    TSpec: ConsensusSpec,
    HotStuffError: From<<TSpec::SyncManager as SyncManager>::Error>,
{
    pub(super) fn new(mode: SyncMode) -> Self {
        Self {
            mode,
            _spec: PhantomData,
        }
    }

    pub(super) async fn on_enter(
        &self,
        context: &mut ConsensusWorkerContext<TSpec>,
    ) -> Result<ConsensusStateEvent, HotStuffError> {
        match self.mode {
            SyncMode::Member { target_epoch } => Self::sync_member(context, target_epoch).await,
            SyncMode::Presync { shard_group } => Self::presync(context, shard_group).await,
        }
    }

    async fn sync_member(
        context: &mut ConsensusWorkerContext<TSpec>,
        target_epoch: Option<Epoch>,
    ) -> Result<ConsensusStateEvent, HotStuffError> {
        // Read from the pre-sync state, which tells which epoch the pool was built in.
        let at_epoch_end = context.hotstuff.new_transactions_at_epoch_end()?;

        context.state_sync.sync(target_epoch).await?;

        // State sync only rewrites the state store, not the consensus transaction pool. Drop any pool
        // records so that on re-entering consensus we don't re-propose transactions the freshly-synced
        // state has already finalised (the rest of the committee removed them on commit, so they can
        // never gather a QC again - this wedges the network). A still-pending transaction is restored
        // to the pool from the record we kept, when a proposal for it arrives
        // (`OnReceiveNewTransaction::resequence_known_transactions`). The transactions this node held
        // unprepared at the end of the epoch it synced past are restored straight away: no proposal will
        // bring them back, because every member of the new shard group might have dropped them.
        let cleared = context.hotstuff.clear_transaction_pool(at_epoch_end).await?;
        if cleared.num_removed > 0 {
            info!(
                target: LOG_TARGET,
                "🧹 Cleared {} transaction(s) from the pool after state sync",
                cleared.num_removed
            );
        }
        if cleared.num_resequenced > 0 {
            info!(
                target: LOG_TARGET,
                "🎱 Sequenced {} transaction(s) held at the end of the previous epoch again",
                cleared.num_resequenced
            );
        }

        Ok(ConsensusStateEvent::SyncComplete)
    }

    /// The transaction pool stays as it is until this node activates, when the member sync clears it before the node
    /// joins consensus.
    ///
    /// One epoch read governs the whole presync, so the checkpoint it syncs to always ends before an epoch in which
    /// this node is not yet a member. A node registered for that epoch returns to `Idle`, which moves it on to the
    /// member sync.
    async fn presync(
        context: &mut ConsensusWorkerContext<TSpec>,
        shard_group: ShardGroup,
    ) -> Result<ConsensusStateEvent, HotStuffError> {
        let epoch = context.epoch_manager.current_epoch().await?;
        if context
            .epoch_manager
            .is_this_validator_registered_for_epoch(epoch)
            .await?
        {
            return Ok(ConsensusStateEvent::PresyncComplete { epoch });
        }
        match context.state_sync.presync(shard_group, epoch).await? {
            PresyncStatus::Synced => Ok(ConsensusStateEvent::PresyncComplete { epoch }),
            PresyncStatus::CheckpointNotAvailable => {
                time::sleep(PRESYNC_RETRY_DELAY).await;
                Ok(ConsensusStateEvent::PresyncDeferred)
            },
        }
    }
}
