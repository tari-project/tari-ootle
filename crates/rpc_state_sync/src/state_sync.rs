//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, time::Instant};

use anyhow::anyhow;
use futures::StreamExt;
use log::*;
use tari_consensus::traits::{ConsensusSpec, SyncManager, SyncStatus};
use tari_consensus_types::LeafBlock;
use tari_epoch_manager::EpochManagerReader;
use tari_ootle_common_types::{
    Epoch,
    ShardGroup,
    VersionedSubstateId,
    VotePower,
    committee::Committee,
    displayable::Displayable,
    optional::Optional,
    shard::Shard,
};
use tari_ootle_p2p::{
    PeerAddress,
    proto::rpc::{GetCheckpointsRequest, GetCheckpointsResponse, SyncStateRequest},
};
use tari_ootle_storage::{
    ShardScopedTreeStoreReader,
    ShardScopedTreeStoreWriter,
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
    consensus_models::{
        BookkeepingModel,
        EpochCheckpoint,
        SubstateRecord,
        SubstateTransition,
        SubstateUpdateBatch,
        SubstateUpdateProof,
        SubstateValueFilterFlags,
    },
};
use tari_rpc_framework::{RpcError, RpcStatusCode};
use tari_state_tree::{SPARSE_MERKLE_PLACEHOLDER_HASH, SpreadPrefixStateTree, SubstateTreeChange, TreeHash, Version};
use tari_validator_node_rpc::{
    STATE_SYNC_MAX_BATCH_SIZE,
    client::{TariValidatorNodeRpcClientFactory, ValidatorNodeClientFactory},
    rpc_service::ValidatorNodeRpcClient,
};

use crate::{error::RpcStateSyncError, stats::StateSyncStats};

const LOG_TARGET: &str = "tari::ootle::rpc_state_sync";

pub struct RpcStateSyncClientProtocol<TConsensusSpec: ConsensusSpec> {
    epoch_manager: TConsensusSpec::EpochManager,
    state_store: TConsensusSpec::StateStore,
    client_factory: TariValidatorNodeRpcClientFactory,
    valid_checkpoints: HashMap<ShardGroup, EpochCheckpoint>,
    stats: StateSyncStats,
}

impl<TConsensusSpec> RpcStateSyncClientProtocol<TConsensusSpec>
where TConsensusSpec: ConsensusSpec<Addr = PeerAddress>
{
    pub fn new(
        epoch_manager: TConsensusSpec::EpochManager,
        state_store: TConsensusSpec::StateStore,
        client_factory: TariValidatorNodeRpcClientFactory,
    ) -> Self {
        Self {
            epoch_manager,
            state_store,
            client_factory,
            valid_checkpoints: HashMap::new(),
            stats: StateSyncStats::default(),
        }
    }

    async fn establish_rpc_session(&self, addr: &PeerAddress) -> Result<ValidatorNodeRpcClient, RpcStateSyncError> {
        let rpc_client = self.client_factory.create_client(addr);
        let client = rpc_client.client_connection().await?;
        Ok(client)
    }

    async fn get_or_fetch_valid_epoch_checkpoint(
        &mut self,
        client: &mut ValidatorNodeRpcClient,
        for_shard_group: ShardGroup,
        prev_committee: &Committee<PeerAddress>,
        prev_epoch: Epoch,
    ) -> Result<Option<EpochCheckpoint>, RpcStateSyncError> {
        let valid_checkpoint = self
            .state_store
            .with_read_tx(|tx| EpochCheckpoint::get_by_shard_group(tx, prev_epoch, for_shard_group))
            .optional()?;

        if let Some(cp) = valid_checkpoint {
            info!(target: LOG_TARGET, "🛜 Checkpoint already fetched and valid: {cp}");
            return Ok(Some(cp));
        }

        self.stats.total_requests += 1;

        match client
            .get_checkpoints(GetCheckpointsRequest {
                from_epoch: Some(prev_epoch.into()),
                num_to_return: 1,
            })
            .await
        {
            Ok(GetCheckpointsResponse { checkpoints }) if checkpoints.is_empty() => Ok(None),
            Ok(GetCheckpointsResponse { mut checkpoints }) => {
                match EpochCheckpoint::try_from(checkpoints.pop().expect("checked is_empty")) {
                    Ok(checkpoint) => {
                        checkpoint.checked_shard_group().map_err(|err| {
                            RpcStateSyncError::InvalidResponse(anyhow!(
                                "Fetched checkpoint for epoch {} has invalid shard group: {err}",
                                checkpoint.epoch()
                            ))
                        })?;
                        info!(target: LOG_TARGET, "🛜 Checkpoint: {checkpoint}");
                        self.validate_checkpoint(&checkpoint, prev_committee, prev_epoch)?;
                        self.state_store.with_write_tx(|tx| checkpoint.save(tx))?;
                        self.valid_checkpoints.insert(for_shard_group, checkpoint.clone());
                        Ok(Some(checkpoint))
                    },
                    Err(err) => Err(RpcStateSyncError::InvalidResponse(err)),
                }
            },
            Err(RpcError::RequestFailed(err)) if err.is_not_found() => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    #[allow(clippy::too_many_lines)]
    async fn start_state_sync(
        &mut self,
        client: &mut ValidatorNodeRpcClient,
        shard: Shard,
        checkpoint: &EpochCheckpoint,
        mut maybe_persisted_state_version: Option<Version>,
    ) -> Result<Option<Version>, RpcStateSyncError> {
        let checkpoint_shard_root = checkpoint.get_shard_root(shard);
        let checkpoint_state_version = checkpoint.get_shard_state_version(shard);

        let initial_local_state_root = self
            .state_store
            .with_read_tx(|tx| self.calculate_state_root_for_shard(tx, shard, maybe_persisted_state_version))?;
        if checkpoint_shard_root == initial_local_state_root {
            info!(target: LOG_TARGET, "Checkpoint state root indicates no further state changes. Nothing to sync for {shard}");
            return Ok(None);
        }

        // We start at 1 because bootstrapped state is at 0
        let start_state_version = maybe_persisted_state_version.unwrap_or(1);
        let mut last_state_version = start_state_version;
        info!(
            target: LOG_TARGET,
            "🛜Syncing from v{start_state_version}",
        );

        self.stats.total_requests += 1;
        let mut state_stream = client
            .sync_state(SyncStateRequest {
                start_state_version,
                shard: shard.as_u32(),
                until_epoch: Some(checkpoint.epoch().into()),
                value_filters: (SubstateValueFilterFlags::all_substates() |
                    SubstateValueFilterFlags::TEMPLATE_METADATA)
                    .bits(),
            })
            .await?;

        let mut tree_changes = vec![];
        let mut updates = vec![];
        let mut expected_state_version = None;

        // syncing states
        while let Some(result) = state_stream.next().await {
            let msg = result?;

            if msg.updates.is_empty() {
                return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                    "Received empty state transition batch."
                )));
            }
            if msg.updates.len() > STATE_SYNC_MAX_BATCH_SIZE {
                return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                    "Received too many state updates in a batch: {}. Expected at most {}.",
                    msg.updates.len(),
                    STATE_SYNC_MAX_BATCH_SIZE
                )));
            }
            if msg.state_version < start_state_version {
                return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                    "Received state version {} that is less than the persisted state version {}.",
                    msg.state_version,
                    start_state_version
                )));
            }

            if expected_state_version.is_some_and(|v| v != msg.state_version) {
                return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                    "Received state version {} that is not the expected state version {}.",
                    msg.state_version,
                    expected_state_version.unwrap()
                )));
            }

            let state_version = msg.state_version;
            if state_version < last_state_version {
                return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                    "Received state version {} that is less than the last state version {}.",
                    state_version,
                    last_state_version
                )));
            }

            last_state_version = state_version;

            self.stats.total_transitions += msg.updates.len() as u64;

            tree_changes.reserve_exact(msg.updates.len());
            updates.reserve_exact(msg.updates.len());

            let updates_for_state_version = msg
                .updates
                .into_iter()
                .map(|t| SubstateUpdateProof::try_from(t).map_err(RpcStateSyncError::InvalidResponse));
            let msg_epoch = msg.epoch.map(Epoch::from).ok_or_else(|| {
                RpcStateSyncError::InvalidResponse(anyhow!("Received state transition with no epoch"))
            })?;

            info!(target: LOG_TARGET, "🛜 Buffering {} state update(s) (state version: v{})", updates_for_state_version.len(), state_version);
            for result in updates_for_state_version {
                let update = result?;
                let tree_change = extract_tree_change(&update)?;

                debug!(target: LOG_TARGET, "🛜 -> state update (v{}) {}", state_version, update);
                tree_changes.push(tree_change);
                updates.push(update);
            }

            info!(target: LOG_TARGET, "🛜 Sync: {} state update(s), state version: v{}", updates.len(), state_version);

            if msg.has_more {
                info!(
                    target: LOG_TARGET,
                    "🛜 Received more state updates for v{}. Continuing to buffer...",
                    state_version
                );
                // Continue buffering
                // TODO: maximum possible state transitions within a single state version?
                expected_state_version = Some(state_version);
                continue;
            }

            expected_state_version = None;

            // Verify and commit changes
            self.state_store.with_write_tx(|tx| {
                info!(
                    target: LOG_TARGET,
                    "🛜 Next state updates batch of size {} from v{}",
                    updates.len(),
                    state_version
                );

                let mut store = ShardScopedTreeStoreWriter::new(tx, shard);

                info!(target: LOG_TARGET, "🛜 {} state update(s) for v{}", updates.len(), state_version);
                self.commit_updates(
                    store.transaction(),
                    shard,
                    msg_epoch,
                    msg.state_version,
                    updates.drain(..),
                )?;

                // Persist tree changes
                if !tree_changes.is_empty() {
                    let mut state_tree = SpreadPrefixStateTree::new(&mut store);
                    info!(target: LOG_TARGET, "🛜 Committing {} state tree changes batch v{}", tree_changes.len(), state_version);
                    let local_state_root = state_tree.batch_put_substate_changes(maybe_persisted_state_version, state_version, tree_changes.drain(..))?;
                    // Only check the state root once we have reached the checkpoint state version
                    // TODO: we should sync to multiple checkpoints to catch misbehaviour earlier
                    if state_version == checkpoint_state_version {
                        if local_state_root != checkpoint_shard_root {
                            error!(
                                target: LOG_TARGET,
                                "❌ State root mismatch for {shard}. Checkpoint {expected} but got {actual}. Rolling back.",
                                expected = checkpoint_shard_root,
                                actual = local_state_root,
                            );

                            // rollback!
                            return Err(RpcStateSyncError::StateRootMismatch {
                                expected: checkpoint_shard_root,
                                actual: local_state_root,
                            });
                        }
                        info!(
                            target: LOG_TARGET,
                            "🛜 ✅ State root for {shard} matches checkpoint: {local_state_root} (v{state_version})",
                        );

                        maybe_persisted_state_version = Some(state_version);
                        store.set_state_version(state_version)?;
                        // Done
                        return Ok(());
                    }

                    maybe_persisted_state_version = Some(state_version);
                    store.set_state_version(state_version)?;
                }

                Ok::<_, RpcStateSyncError>(())
            })?;
        }

        info!(target: LOG_TARGET, "🛜 Synced state for {shard} to v{}", maybe_persisted_state_version.unwrap_or(1));

        Ok(maybe_persisted_state_version)
    }

    fn calculate_state_root_for_shard(
        &self,
        tx: &<TConsensusSpec::StateStore as StateStore>::ReadTransaction<'_>,
        shard: Shard,
        version: Option<Version>,
    ) -> Result<TreeHash, RpcStateSyncError> {
        let Some(version) = version else {
            return Ok(SPARSE_MERKLE_PLACEHOLDER_HASH);
        };
        let mut store = ShardScopedTreeStoreReader::new(tx, shard);
        let state_tree = SpreadPrefixStateTree::new(&mut store);
        let root = state_tree.get_root_hash(version)?;
        Ok(root)
    }

    pub fn commit_updates<TTx: StateStoreWriteTransaction, I: IntoIterator<Item = SubstateUpdateProof>>(
        &self,
        tx: &mut TTx,
        shard: Shard,
        epoch: Epoch,
        state_version: Version,
        updates: I,
    ) -> Result<(), StorageError> {
        let mut batch = SubstateUpdateBatch::new(epoch);

        batch
            .with_transition(shard, state_version)
            .extend(updates.into_iter().map(|update| match update {
                SubstateUpdateProof::Create(create) => SubstateTransition::Up {
                    id: create.substate.substate_id,
                    version: create.substate.version,
                    substate_or_hash: create.substate.value,
                },
                SubstateUpdateProof::Destroy(destroy) => SubstateTransition::Down {
                    id: VersionedSubstateId::new(destroy.substate_id, destroy.version),
                },
            }));

        SubstateRecord::commit_batch(tx, batch)?;

        Ok(())
    }

    async fn get_sync_committees(
        &self,
        local_shard_group: ShardGroup,
        current_epoch: Epoch,
    ) -> Result<HashMap<ShardGroup, Committee<PeerAddress>>, RpcStateSyncError> {
        // We are behind at least one epoch.
        // We get the current substate range, and we ask committees from previous epoch in this range to give us
        // data.
        let prev_epoch = current_epoch
            .checked_sub(Epoch(1))
            .ok_or_else(|| RpcStateSyncError::NoCommittees(Epoch::zero()))?;
        info!(target: LOG_TARGET,"Previous epoch is {}", prev_epoch);
        // We want to get any committees from the previous epoch that overlap with our shard group in this epoch
        let committees = self
            .epoch_manager
            .get_committees_overlapping_shard_group(prev_epoch, local_shard_group)
            .await?;

        if committees.is_empty() {
            return Err(RpcStateSyncError::NoCommittees(prev_epoch));
        }

        let committees = committees.into_iter().collect::<HashMap<_, _>>();
        info!(target: LOG_TARGET, "🛜 Querying {} committee(s) from epoch {}", committees.len(), prev_epoch);
        Ok(committees)
    }

    fn validate_checkpoint(
        &self,
        checkpoint: &EpochCheckpoint,
        committee: &Committee<PeerAddress>,
        epoch: Epoch,
    ) -> Result<(), RpcStateSyncError> {
        let quorum_threshold = committee.quorum_threshold();
        checkpoint
            .validate(epoch, quorum_threshold, |pk| {
                Ok(committee.get_power_by_public_key(pk).unwrap_or_else(VotePower::zero))
            })
            .map_err(|err| RpcStateSyncError::InvalidResponse(anyhow!("Checkpoint is not valid: {err}",)))?;

        info!(
            target: LOG_TARGET,
            "🛜 ✅ Checkpoint {} is valid",
            checkpoint,
        );

        Ok(())
    }

    async fn should_treat_missing_prev_epoch_checkpoint_as_bootstrap(
        &self,
        current_epoch: Epoch,
        our_start_epoch: Epoch,
        prev_epoch_committees: &HashMap<ShardGroup, Committee<PeerAddress>>,
    ) -> Result<bool, RpcStateSyncError> {
        let Some(prev_epoch) = current_epoch.checked_sub(Epoch(1)) else {
            return Ok(false);
        };

        if our_start_epoch != prev_epoch {
            return Ok(false);
        }

        let local_epoch = self.state_store.with_read_tx(|tx| tx.current_epoch())?;
        if local_epoch >= prev_epoch {
            return Ok(false);
        }

        let previous_epoch_vns = self.epoch_manager.get_all_validator_nodes(prev_epoch).await?;
        let start_epochs = previous_epoch_vns
            .into_iter()
            .map(|vn| (vn.public_key, vn.start_epoch))
            .collect::<HashMap<_, _>>();

        let all_members_started_in_prev_epoch = prev_epoch_committees
            .values()
            .flat_map(|committee| committee.iter())
            .all(|member| start_epochs.get(&member.public_key) == Some(&prev_epoch));

        if all_members_started_in_prev_epoch {
            info!(
                target: LOG_TARGET,
                "🛜 Previous epoch {} consisted entirely of newly activated validators and local state is still at \
                 epoch {}. Missing checkpoints will be treated as first-committee bootstrap.",
                prev_epoch,
                local_epoch
            );
        }

        Ok(all_members_started_in_prev_epoch)
    }

    fn is_checkpoint_temporarily_unavailable_error(err: &RpcStateSyncError, prev_epoch: Epoch) -> bool {
        match err {
            RpcStateSyncError::CheckpointNotAvailable { epoch } => *epoch == prev_epoch,
            RpcStateSyncError::RpcError(RpcError::RequestFailed(status)) => {
                let details = status.details();
                (status.as_status_code() == RpcStatusCode::BadRequest &&
                    details.contains(&format!("Peer requested checkpoint with epoch {}", prev_epoch))) ||
                    (status.as_status_code() == RpcStatusCode::General &&
                        (details.contains("Consensus is not running on this node") ||
                            details.contains("Node is still catching up to the epoch") ||
                            details.contains("Node is not in sync with the consensus epoch")))
            },
            _ => false,
        }
    }

    /// Synchronizes the given [`Shard`].
    async fn sync_shard(
        &mut self,
        shard: Shard,
        shard_group: ShardGroup,
        epoch: Epoch,
        prev_committee: &Committee<PeerAddress>,
        our_vn_addr: &PeerAddress,
        allow_committee_bootstrap: bool,
    ) -> Result<Option<Version>, RpcStateSyncError> {
        let prev_epoch = epoch
            .checked_sub(Epoch(1))
            .ok_or_else(|| RpcStateSyncError::InvalidResponse(anyhow!("Epoch is zero")))?;
        info!(target: LOG_TARGET, "🛜 Syncing state for shard {shard} and epoch {}", prev_epoch);

        let mut last_hard_error = None;
        let mut saw_unavailable_checkpoint = false;

        for member in prev_committee.shuffled() {
            if *our_vn_addr == member.address {
                continue;
            }
            let mut client = match self.establish_rpc_session(&member.address).await {
                Ok(c) => c,
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        "Failed to establish RPC session with vn {member}: {err}. Attempting another VN if available"
                    );
                    last_hard_error = Some(err);
                    continue;
                },
            };

            // fetch checkpoint
            let checkpoint = match self
                .get_or_fetch_valid_epoch_checkpoint(&mut client, shard_group, prev_committee, prev_epoch)
                .await
            {
                Ok(Some(cp)) => cp,
                Ok(None) => {
                    warn!(
                        target: LOG_TARGET,
                        "❓️ No checkpoint for epoch {prev_epoch} from {member}. Previous committee exists, so state \
                         sync will retry instead of proceeding without a checkpoint.",
                    );
                    saw_unavailable_checkpoint = true;
                    continue;
                },
                Err(err) => {
                    if Self::is_checkpoint_temporarily_unavailable_error(&err, prev_epoch) {
                        saw_unavailable_checkpoint = true;
                        warn!(
                            target: LOG_TARGET,
                            "⚠️Checkpoint for epoch {prev_epoch} is not yet available from {member}: {err}. \
                             Attempting another peer if available"
                        );
                        continue;
                    }

                    warn!(
                        target: LOG_TARGET,
                        "⚠️Failed to fetch checkpoint from {member}: {err}. Attempting another peer if available"
                    );
                    last_hard_error = Some(err);
                    continue;
                },
            };

            let maybe_persisted_state_version = self
                .state_store
                .with_read_tx(|tx| tx.state_tree_versions_get_latest(shard))?;

            match self
                .start_state_sync(&mut client, shard, &checkpoint, maybe_persisted_state_version)
                .await
            {
                Ok(maybe_version) => {
                    return Ok(maybe_version);
                },
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        "⚠️Failed to sync state from {member}: {err}. Attempting another peer if available"
                    );
                    last_hard_error = Some(err);
                    continue;
                },
            }
        }

        if allow_committee_bootstrap && saw_unavailable_checkpoint && last_hard_error.is_none() {
            info!(
                target: LOG_TARGET,
                "🛜 No checkpoint for epoch {prev_epoch} is available yet from the freshly activated previous \
                 committee. Treating this as first-committee bootstrap for shard {shard}."
            );
            return Ok(None);
        }

        if saw_unavailable_checkpoint && last_hard_error.is_none() {
            return Err(RpcStateSyncError::CheckpointNotAvailable { epoch: prev_epoch });
        }

        if let Some(err) = last_hard_error {
            return Err(err);
        }

        Err(RpcStateSyncError::SyncFailedAllPeers {
            committee_size: prev_committee.len(),
        })
    }

    async fn sync_global_shard(
        &mut self,
        current_epoch: Epoch,
        shard_group: ShardGroup,
        prev_committees: &HashMap<ShardGroup, Committee<PeerAddress>>,
        our_vn_address: &PeerAddress,
        allow_committee_bootstrap: bool,
    ) -> Result<Option<Version>, RpcStateSyncError> {
        let mut last_error = None;

        for (sg, prev_committee) in prev_committees {
            // TODO: any checkpoint for the previous epoch will justify the global shard sync.
            //       Currently we'll fetch the checkpoint again even if we already have it if there are more than one
            // shard groups.
            let result = self
                .sync_shard(
                    Shard::global(),
                    shard_group,
                    current_epoch,
                    prev_committee,
                    our_vn_address,
                    allow_committee_bootstrap,
                )
                .await;
            match result {
                Ok(maybe_version) => {
                    let Some(version) = maybe_version else {
                        info!(target: LOG_TARGET, "🛜 No state changes for global shard");
                        return Ok(None);
                    };
                    info!(target: LOG_TARGET, "🛜 Synced global shard to v{}", version);
                    return Ok(Some(version));
                },
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        "⚠️ Failed to sync global shard from {sg}: {err}. Attempting another committee if available"
                    );
                    last_error = Some(err);
                },
            }
        }

        if let Some(err) = last_error {
            return Err(err);
        }

        Err(RpcStateSyncError::SyncFailedAllPeers {
            committee_size: prev_committees.len(),
        })
    }

    async fn sync_inner(&mut self) -> Result<(), RpcStateSyncError> {
        let timer = Instant::now();
        let current_epoch = self.epoch_manager.current_epoch().await?;
        let our_vn = self.epoch_manager.get_our_validator_node(current_epoch).await?;
        let local_info = self.epoch_manager.get_local_committee_info(current_epoch).await?;
        let prev_epoch_committees = match self.get_sync_committees(local_info.shard_group(), current_epoch).await {
            Ok(committees) => committees,
            Err(RpcStateSyncError::NoCommittees(prev_epoch)) => {
                info!(target: LOG_TARGET, "No committees for the previous epoch {prev_epoch}. This is the first committee.");
                return Ok(());
            },
            Err(err) => return Err(err),
        };

        // Edge case: we're the only VN in a previous committee
        if prev_epoch_committees.len() == 1 &&
            prev_epoch_committees.values().all(|committee| {
                committee.len() == 1 && committee.address_iter().all(|addr| *addr == our_vn.address)
            })
        {
            info!(target: LOG_TARGET, "This node is the only Validator in the previous committee - no need to sync.");
            return Ok(());
        }

        let allow_committee_bootstrap = self
            .should_treat_missing_prev_epoch_checkpoint_as_bootstrap(
                current_epoch,
                our_vn.start_epoch,
                &prev_epoch_committees,
            )
            .await?;

        let local_shard_group = local_info.shard_group();

        self.sync_global_shard(
            current_epoch,
            ShardGroup::all_shards(local_info.num_preshards()),
            &prev_epoch_committees,
            &our_vn.address,
            allow_committee_bootstrap,
        )
        .await?;

        // Sync data from each committee in range of the committee we're joining.
        // NOTE: we don't have to worry about substates in address range because shard boundaries are fixed.
        for (shard_group, committee) in prev_epoch_committees {
            let Some(intersect_shard_group) = shard_group.intersection(&local_shard_group) else {
                warn!(
                    target: LOG_TARGET,
                    "❗️ Shard group {shard_group} does not intersect with our shard group {local_shard_group}. Skipping."
                );
                continue;
            };
            for shard in intersect_shard_group.shard_iter() {
                self.sync_shard(
                    shard,
                    shard_group,
                    current_epoch,
                    &committee,
                    &our_vn.address,
                    allow_committee_bootstrap,
                )
                    .await?;
            }
        }

        self.stats.total_time = timer.elapsed();
        Ok(())
    }
}

impl<TConsensusSpec> SyncManager for RpcStateSyncClientProtocol<TConsensusSpec>
where TConsensusSpec: ConsensusSpec<Addr = PeerAddress> + Send + Sync + 'static
{
    type Error = RpcStateSyncError;

    async fn check_sync(&self) -> Result<SyncStatus, Self::Error> {
        let current_epoch = self.epoch_manager.current_epoch().await?;

        let leaf_block = self
            .state_store
            .with_read_tx(|tx| LeafBlock::get(tx, current_epoch).optional())?;

        // We only sync if we're behind by an epoch. The current epoch is replayed in consensus.
        if current_epoch > leaf_block.map_or(Epoch::zero(), |b| b.epoch()) {
            info!(target: LOG_TARGET, "🛜Our current leaf block {} is behind the current epoch {}. Syncing...", leaf_block.display(), current_epoch);
            return Ok(SyncStatus::Behind);
        }

        Ok(SyncStatus::UpToDate)
    }

    async fn sync(&mut self) -> Result<(), Self::Error> {
        if let Err(err) = self.sync_inner().await {
            warn!(target: LOG_TARGET, "🛜State sync failed: {err} (stats: {})", self.stats);
            // Clear the valid checkpoints cache
            self.valid_checkpoints = HashMap::new();
            self.stats = StateSyncStats::default();
            return Err(err);
        }

        info!(target: LOG_TARGET, "🛜State sync completed successfully: {}", self.stats);

        // Clear the valid checkpoints cache
        self.valid_checkpoints = HashMap::new();
        self.stats = StateSyncStats::default();
        Ok(())
    }
}

fn extract_tree_change(update: &SubstateUpdateProof) -> Result<SubstateTreeChange, RpcStateSyncError> {
    match update {
        SubstateUpdateProof::Create(create) => {
            let id = create.substate.as_versioned_substate_id_ref();
            Ok(SubstateTreeChange::Up {
                id: id.to_owned(),
                value_hash: create.substate.to_value_hash(),
            })
        },
        SubstateUpdateProof::Destroy(destroy) => Ok(SubstateTreeChange::Down {
            id: destroy.to_versioned_substate_id(),
        }),
    }
}
