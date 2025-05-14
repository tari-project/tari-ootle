//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{cmp, collections::HashMap, time::Instant};

use anyhow::anyhow;
use futures::StreamExt;
use indexmap::IndexMap;
use log::*;
use tari_consensus::{
    hotstuff::substate_store::{ShardScopedTreeStoreReader, ShardScopedTreeStoreWriter},
    traits::{ConsensusSpec, SyncManager, SyncStatus},
};
use tari_dan_common_types::{
    committee::Committee,
    displayable::Displayable,
    optional::Optional,
    shard::Shard,
    Epoch,
    PeerAddress,
    ShardGroup,
    VersionedSubstateId,
};
use tari_dan_p2p::proto::rpc::{GetCheckpointRequest, GetCheckpointResponse, SyncStateRequest};
use tari_dan_storage::{
    consensus_models::{
        BlockId,
        EpochCheckpoint,
        EpochStateRoot,
        LeafBlock,
        QcId,
        StateTransition,
        StateTransitionId,
        SubstateCreatedProof,
        SubstateDestroyedProof,
        SubstateRecord,
        SubstateUpdate,
    },
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};
use tari_epoch_manager::EpochManagerReader;
use tari_rpc_framework::RpcError;
use tari_state_tree::{
    compute_merkle_root_for_hashes,
    SpreadPrefixStateTree,
    SubstateTreeChange,
    TreeHash,
    Version,
    SPARSE_MERKLE_PLACEHOLDER_HASH,
};
use tari_template_manager::interface::{TemplateChange, TemplateManagerHandle};
use tari_validator_node_rpc::{
    client::{TariValidatorNodeRpcClientFactory, ValidatorNodeClientFactory},
    rpc_service::ValidatorNodeRpcClient,
};

use crate::{error::RpcStateSyncError, stats::StateSyncStats};

const BATCH_SIZE: usize = 100;
const LOG_TARGET: &str = "tari::dan::comms_rpc_state_sync";

pub struct RpcStateSyncClientProtocol<TConsensusSpec: ConsensusSpec> {
    epoch_manager: TConsensusSpec::EpochManager,
    state_store: TConsensusSpec::StateStore,
    client_factory: TariValidatorNodeRpcClientFactory,
    template_manager: TemplateManagerHandle,
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
        template_manager: TemplateManagerHandle,
    ) -> Self {
        Self {
            epoch_manager,
            state_store,
            client_factory,
            template_manager,
            valid_checkpoints: HashMap::new(),
            stats: StateSyncStats::default(),
        }
    }

    async fn establish_rpc_session(&self, addr: &PeerAddress) -> Result<ValidatorNodeRpcClient, RpcStateSyncError> {
        let mut rpc_client = self.client_factory.create_client(addr);
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
        if let Some(cp) = self.valid_checkpoints.get(&for_shard_group) {
            info!(target: LOG_TARGET, "🛜 Checkpoint already fetched and valid: {cp}");
            return Ok(Some(cp.clone()));
        }

        self.stats.total_requests += 1;

        match client
            .get_checkpoint(GetCheckpointRequest {
                epoch: prev_epoch.as_u64(),
            })
            .await
        {
            Ok(GetCheckpointResponse {
                checkpoint: Some(checkpoint),
            }) => match EpochCheckpoint::try_from(checkpoint) {
                Ok(checkpoint) => {
                    info!(target: LOG_TARGET, "🛜 Checkpoint: {checkpoint}");
                    self.validate_checkpoint(&checkpoint, prev_committee, prev_epoch)?;
                    self.valid_checkpoints.insert(for_shard_group, checkpoint.clone());
                    Ok(Some(checkpoint))
                },
                Err(err) => Err(RpcStateSyncError::InvalidResponse(err)),
            },
            Err(RpcError::RequestFailed(err)) if err.is_not_found() => Ok(None),
            Ok(GetCheckpointResponse { checkpoint: None }) => Ok(None),
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
        template_changes_mut: &mut Vec<TemplateChange>,
    ) -> Result<Option<Version>, RpcStateSyncError> {
        let checkpoint_state_root = checkpoint.get_shard_root(shard);
        if checkpoint_state_root == SPARSE_MERKLE_PLACEHOLDER_HASH {
            info!(target: LOG_TARGET, "Checkpoint state root indicates no state changes. Nothing to sync for {shard}");
            return Ok(None);
        }

        let checkpoint_block_id = BlockId::new(checkpoint.header().calculate_block_id());

        let current_epoch = self.epoch_manager.current_epoch().await?;

        let last_state_transition_id = self
            .state_store
            .with_read_tx(|tx| StateTransition::get_last_id(tx, shard))
            .optional()?
            .unwrap_or_else(|| StateTransitionId::initial(shard));

        let persisted_version = self
            .state_store
            .with_read_tx(|tx| tx.state_tree_versions_get_latest(shard))?;

        if current_epoch == last_state_transition_id.epoch() {
            info!(target: LOG_TARGET, "🛜Already up to date. No need to sync.");
            return Ok(persisted_version);
        }

        let mut maybe_current_version = persisted_version;
        let current_version = maybe_current_version.unwrap_or(0);

        info!(
            target: LOG_TARGET,
            "🛜Syncing from v{} to state transition {last_state_transition_id}",
            current_version
        );

        self.stats.total_requests += 1;
        let mut state_stream = client
            .sync_state(SyncStateRequest {
                start_epoch: last_state_transition_id.epoch().as_u64(),
                start_shard: last_state_transition_id.shard().as_u32(),
                start_seq: last_state_transition_id.seq(),
                current_epoch: current_epoch.as_u64(),
            })
            .await?;

        let mut tree_changes = vec![];

        // syncing states
        while let Some(result) = state_stream.next().await {
            let msg = match result {
                Ok(msg) => msg,
                Err(err) if err.is_not_found() => {
                    return Ok(maybe_current_version);
                },
                Err(err) => {
                    return Err(err.into());
                },
            };

            if msg.transitions.is_empty() {
                return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                    "Received empty state transition batch."
                )));
            }

            self.stats.total_transitions += msg.transitions.len() as u64;

            tree_changes.reserve_exact(cmp::min(msg.transitions.len(), BATCH_SIZE));

            self.state_store.with_write_tx(|tx| {
                info!(
                    target: LOG_TARGET,
                    "🛜 Next state updates batch of size {} from v{}",
                    msg.transitions.len(),
                    current_version
                );

                let mut store = ShardScopedTreeStoreWriter::new(tx, shard);

                for transition in msg.transitions {
                    let transition =
                        StateTransition::try_from(transition).map_err(RpcStateSyncError::InvalidResponse)?;
                    if transition.id.shard() != shard {
                        return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                            "Received state transition for shard {} which is not the expected shard {}.",
                            transition.id.shard(),
                            shard
                        )));
                    }

                    if transition.id.epoch().is_zero() {
                        return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                            "Received state transition with epoch 0."
                        )));
                    }

                    if transition.id.epoch() >= current_epoch {
                        return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                            "Received state transition for epoch {} which is at or ahead of our current epoch {}.",
                            transition.id.epoch(),
                            current_epoch
                        )));
                    }

                    let change = match &transition.update {
                        SubstateUpdate::Create(create) => {
                            let id = create.substate.as_versioned_substate_id_ref();
                            if let Some(template_address) = create.substate.substate_id.as_template() {
                                match create
                                    .substate
                                    .value
                                    .value() {
                                    Some(value) => {
                                        let template = value.as_template()
                                            .ok_or_else(|| RpcStateSyncError::InvalidResponse(
                                                anyhow!("Validator returned a template address {} but substate value was not a template", id.substate_id())
                                            ))?;

                                        info!(target: LOG_TARGET, "🛜 Add template {id}");
                                        template_changes_mut.push(TemplateChange::Add {
                                            template_address,
                                            author_public_key: template.author,
                                            binary_hash: template.binary_hash.into_array().into(),
                                            epoch: transition.id.epoch(),
                                        });
                                    }
                                    None => {
                                        // TODO: currently you cannot DOWN a template. If we were to allow deprecations, it would likely be marking the template as deprecated rather than DOWNing it, and not permitting any template (non-component) calls to the template.
                                        // We could still handle this case by requesting the template by address and verifying the template address hash i.e. peers send author and binary.
                                        warn!(target: LOG_TARGET, "❗️ NEVER HAPPEN: Validator sent us a template {} that has no value, indicating it will be DOWNed later. We are not able to sync it", id);
                                    }
                                };
                            }

                            SubstateTreeChange::Up {
                                id: id.to_owned(),
                                value_hash: create.substate.to_value_hash(),
                            }
                        }
                        SubstateUpdate::Destroy(destroy) => {
                            if let Some(template_address) = destroy.substate_id.as_template() {
                                info!(target: LOG_TARGET, "🛜 Deprecate template {}", template_address);
                                template_changes_mut.push(TemplateChange::Deprecate { template_address });
                            }

                            SubstateTreeChange::Down {
                                id: destroy.to_versioned_substate_id()
                            }
                        }
                    };



                    info!(target: LOG_TARGET, "🛜 Applying state update (v{}) {}", current_version, transition);
                    self.commit_update(store.transaction(), checkpoint, checkpoint_block_id, transition)?;

                    tree_changes.push(change);
                    if tree_changes.len() == BATCH_SIZE {
                        let mut state_tree = SpreadPrefixStateTree::new(&mut store);
                        let next_version = current_version + 1;
                        info!(target: LOG_TARGET, "🛜 Committing {} state tree changes v{} to v{}", tree_changes.len(), current_version, next_version);
                        state_tree.batch_put_substate_changes(maybe_current_version, next_version, tree_changes.drain(..))?;
                        maybe_current_version = Some(next_version);
                        store.set_version(next_version)?;
                    }
                }

                if !tree_changes.is_empty() {
                    let mut state_tree = SpreadPrefixStateTree::new(&mut store);
                    let next_version = current_version + 1;
                    info!(target: LOG_TARGET, "🛜 Committing final {} state tree changes v{} to v{}", tree_changes.len(), current_version, next_version);
                    state_tree.batch_put_substate_changes(maybe_current_version, next_version, tree_changes.drain(..))?;
                    maybe_current_version = Some(next_version);
                    store.set_version(next_version)?;
                }

                Ok::<_, RpcStateSyncError>(())
            })?;
        }

        let local_state_root = self.calculate_state_root_for_shard(shard, maybe_current_version)?;
        if local_state_root != checkpoint_state_root {
            error!(
                target: LOG_TARGET,
                "❌State root mismatch for {shard}. Checkpoint {expected} but got {actual}. Rolling back.",
                expected = checkpoint_state_root,
                actual = local_state_root,
            );

            // TODO: rollback
            return Err(RpcStateSyncError::StateRootMismatch {
                expected: checkpoint_state_root,
                actual: local_state_root,
            });
        }

        info!(target: LOG_TARGET, "🛜 Synced state for {shard} to v{} with root {local_state_root}", maybe_current_version.unwrap_or(0));

        Ok(maybe_current_version)
    }

    fn calculate_state_root_for_shard(
        &self,
        shard: Shard,
        version: Option<Version>,
    ) -> Result<TreeHash, RpcStateSyncError> {
        let Some(version) = version else {
            return Ok(SPARSE_MERKLE_PLACEHOLDER_HASH);
        };
        self.state_store.with_read_tx(|tx| {
            let mut store = ShardScopedTreeStoreReader::new(tx, shard);
            let state_tree = SpreadPrefixStateTree::new(&mut store);
            let root = state_tree.get_root_hash(version)?;
            Ok(root)
        })
    }

    pub fn commit_update<TTx: StateStoreWriteTransaction>(
        &self,
        tx: &mut TTx,
        checkpoint: &EpochCheckpoint,
        checkpoint_block_id: BlockId,
        transition: StateTransition,
    ) -> Result<(), StorageError> {
        match transition.update {
            SubstateUpdate::Create(SubstateCreatedProof { substate }) => {
                SubstateRecord::new(
                    substate.substate_id,
                    substate.version,
                    substate.value,
                    transition.id.shard(),
                    transition.id.epoch(),
                    checkpoint_block_id,
                    // TODO: correct QC ID
                    QcId::zero(),
                )
                .create(tx)?;
            },
            SubstateUpdate::Destroy(SubstateDestroyedProof { substate_id, version }) => {
                SubstateRecord::destroy(
                    tx,
                    VersionedSubstateId::new(substate_id, version),
                    transition.id.shard(),
                    transition.id.epoch(),
                    checkpoint.header().height.into(),
                    // TODO
                    &QcId::zero(),
                )?;
            },
        }

        Ok(())
    }

    async fn get_sync_committees(
        &self,
        local_shard_group: ShardGroup,
        current_epoch: Epoch,
    ) -> Result<HashMap<ShardGroup, Committee<PeerAddress>>, RpcStateSyncError> {
        // We are behind at least one epoch.
        // We get the current substate range, and we asks committees from previous epoch in this range to give us
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
            .validate(epoch, quorum_threshold, |pk| Ok(committee.contains_public_key(pk)))
            .map_err(|err| RpcStateSyncError::InvalidResponse(anyhow!("Checkpoint is not valid: {err}",)))?;

        info!(
            target: LOG_TARGET,
            "🛜 ✅ Checkpoint {} is valid",
            checkpoint,
        );

        Ok(())
    }

    /// Synchronizes the given [`Shard`].
    pub async fn sync_shard(
        &mut self,
        shard: Shard,
        shard_group: ShardGroup,
        epoch: Epoch,
        prev_committee: &Committee<PeerAddress>,
        our_vn_addr: &PeerAddress,
    ) -> Result<Option<Version>, RpcStateSyncError> {
        let mut remaining_members = prev_committee.len();

        info!(target: LOG_TARGET, "🛜 Syncing state for shard {shard} and epoch {}", epoch.saturating_sub(Epoch(1)));
        for (addr, _) in prev_committee.shuffled() {
            remaining_members = remaining_members.saturating_sub(1);
            if our_vn_addr == addr {
                continue;
            }
            let mut client = match self.establish_rpc_session(addr).await {
                Ok(c) => c,
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        "Failed to establish RPC session with vn {addr}: {err}. Attempting another VN if available"
                    );
                    if remaining_members == 0 {
                        return Err(err);
                    }
                    continue;
                },
            };

            // fetch checkpoint
            // TODO: NB refactor to fetch the checkpoint once for the shard group - instead of for each shard and each
            // attempt - once it's validated, there is no need to fetch it again
            let prev_epoch = epoch
                .checked_sub(Epoch(1))
                .ok_or_else(|| RpcStateSyncError::InvalidResponse(anyhow!("Epoch is zero")))?;
            let checkpoint = match self
                .get_or_fetch_valid_epoch_checkpoint(&mut client, shard_group, prev_committee, prev_epoch)
                .await
            {
                Ok(Some(cp)) => cp,
                Ok(None) => {
                    // TODO: we should check with f + 1 validators in this case. If a single validator reports
                    // this falsely, this will prevent us from continuing with consensus for a long time (state
                    // root will mismatch).
                    // TODO: we should instead ask the epoch manager if this is the first epoch in the network
                    warn!(
                        target: LOG_TARGET,
                        "❓No checkpoint for epoch {epoch}. This may mean that this is the first epoch in the network"
                    );
                    return Ok(None);
                },
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        "⚠️Failed to fetch checkpoint from {addr}: {err}. Attempting another peer if available"
                    );
                    if remaining_members == 0 {
                        return Err(err);
                    }
                    continue;
                },
            };

            let mut template_changes = vec![];

            match self
                .start_state_sync(&mut client, shard, &checkpoint, &mut template_changes)
                .await
            {
                Ok(maybe_version) => {
                    // We only enqueue these if state sync succeeds and the state root matches
                    if !template_changes.is_empty() {
                        self.template_manager.enqueue_template_changes(template_changes).await?;
                    }
                    return Ok(maybe_version);
                },
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        "⚠️Failed to sync state from {addr}: {err}. Attempting another peer if available"
                    );

                    if remaining_members == 0 {
                        return Err(err);
                    }
                    continue;
                },
            }
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
    ) -> Result<Option<Version>, RpcStateSyncError> {
        let mut last_error = None;

        for (sg, prev_committee) in prev_committees {
            let result = self
                .sync_shard(
                    Shard::global(),
                    shard_group,
                    current_epoch,
                    prev_committee,
                    our_vn_address,
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
        let local_shard_group = local_info.shard_group();

        let mut shard_state_roots = IndexMap::with_capacity(local_shard_group.len() + 1);

        let maybe_version = self
            .sync_global_shard(
                current_epoch,
                ShardGroup::all_shards(local_info.num_preshards()),
                &prev_epoch_committees,
                &our_vn.address,
            )
            .await?;
        let local_state_root = self.calculate_state_root_for_shard(Shard::global(), maybe_version)?;
        shard_state_roots.insert(Shard::global(), local_state_root);

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
                let maybe_current_version = self
                    .sync_shard(shard, shard_group, current_epoch, &committee, &our_vn.address)
                    .await?;
                let local_state_root = self.calculate_state_root_for_shard(shard, maybe_current_version)?;
                shard_state_roots.insert_sorted(shard, local_state_root);
            }
        }

        // Calculate the shard group merkle root and save it for the next genesis
        let final_state_root = compute_merkle_root_for_hashes(shard_state_roots.into_values())?;
        self.state_store
            .with_write_tx(|tx| EpochStateRoot::new(current_epoch, local_shard_group, final_state_root).set(tx))?;

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
            warn!(target: LOG_TARGET, "🛜State sync failed: {err}");
            // Clear the valid checkpoints cache
            self.valid_checkpoints = HashMap::new();
            return Err(err);
        }

        // Clear the valid checkpoints cache
        self.valid_checkpoints = HashMap::new();

        info!(target: LOG_TARGET, "🛜State sync complete: {}", self.stats);
        self.stats = StateSyncStats::default();
        Ok(())
    }
}
