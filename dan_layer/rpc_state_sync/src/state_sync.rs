//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::cmp;

use anyhow::anyhow;
use futures::StreamExt;
use log::*;
use tari_consensus::{
    hotstuff::substate_store::{ShardScopedTreeStoreReader, ShardScopedTreeStoreWriter},
    traits::{ConsensusSpec, SyncManager, SyncStatus},
};
use tari_dan_common_types::{
    committee::Committee,
    optional::Optional,
    shard::Shard,
    Epoch,
    NodeHeight,
    PeerAddress,
    ShardGroup,
    VersionedSubstateId,
};
use tari_dan_p2p::proto::rpc::{GetCheckpointRequest, GetCheckpointResponse, SyncStateRequest};
use tari_dan_storage::{
    consensus_models::{
        EpochCheckpoint,
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
use tari_state_tree::{SpreadPrefixStateTree, SubstateTreeChange, TreeHash, Version, SPARSE_MERKLE_PLACEHOLDER_HASH};
use tari_template_manager::interface::{TemplateChange, TemplateManagerHandle};
use tari_validator_node_rpc::{
    client::{TariValidatorNodeRpcClientFactory, ValidatorNodeClientFactory},
    rpc_service::ValidatorNodeRpcClient,
};

use crate::error::CommsRpcConsensusSyncError;

const BATCH_SIZE: usize = 100;
const LOG_TARGET: &str = "tari::dan::comms_rpc_state_sync";

pub struct RpcStateSyncClientProtocol<TConsensusSpec: ConsensusSpec> {
    epoch_manager: TConsensusSpec::EpochManager,
    state_store: TConsensusSpec::StateStore,
    client_factory: TariValidatorNodeRpcClientFactory,
    template_manager: TemplateManagerHandle,
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
        }
    }

    async fn establish_rpc_session(
        &self,
        addr: &PeerAddress,
    ) -> Result<ValidatorNodeRpcClient, CommsRpcConsensusSyncError> {
        let mut rpc_client = self.client_factory.create_client(addr);
        let client = rpc_client.client_connection().await?;
        Ok(client)
    }

    async fn fetch_epoch_checkpoint(
        &self,
        client: &mut ValidatorNodeRpcClient,
        current_epoch: Epoch,
    ) -> Result<Option<EpochCheckpoint>, CommsRpcConsensusSyncError> {
        match client
            .get_checkpoint(GetCheckpointRequest {
                current_epoch: current_epoch.as_u64(),
            })
            .await
        {
            Ok(GetCheckpointResponse {
                checkpoint: Some(checkpoint),
            }) => match EpochCheckpoint::try_from(checkpoint) {
                Ok(cp) => Ok(Some(cp)),
                Err(err) => Err(CommsRpcConsensusSyncError::InvalidResponse(err)),
            },
            Err(RpcError::RequestFailed(err)) if err.is_not_found() => Ok(None),
            Ok(GetCheckpointResponse { checkpoint: None }) => Ok(None),
            Err(RpcError::RequestFailed(err)) if err.is_not_found() => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    #[allow(clippy::too_many_lines)]
    async fn start_state_sync(
        &self,
        client: &mut ValidatorNodeRpcClient,
        shard: Shard,
        checkpoint: &EpochCheckpoint,
        template_changes_mut: &mut Vec<TemplateChange>,
    ) -> Result<Option<Version>, CommsRpcConsensusSyncError> {
        let checkpoint_state_root = checkpoint.get_shard_root(shard);
        if checkpoint_state_root == SPARSE_MERKLE_PLACEHOLDER_HASH {
            info!(target: LOG_TARGET, "Checkpoint state root indicates no state changes. Nothing to sync for {shard}");
            return Ok(None);
        }

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

        let mut current_version = persisted_version;

        info!(
            target: LOG_TARGET,
            "🛜Syncing from v{} to state transition {last_state_transition_id}",
            current_version.unwrap_or(0),
        );

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
                    return Ok(current_version);
                },
                Err(err) => {
                    return Err(err.into());
                },
            };

            if msg.transitions.is_empty() {
                return Err(CommsRpcConsensusSyncError::InvalidResponse(anyhow!(
                    "Received empty state transition batch."
                )));
            }

            tree_changes.reserve_exact(cmp::min(msg.transitions.len(), BATCH_SIZE));

            self.state_store.with_write_tx(|tx| {
                info!(
                    target: LOG_TARGET,
                    "🛜 Next state updates batch of size {} from v{}",
                    msg.transitions.len(),
                    current_version.unwrap_or(0),
                );

                let mut store = ShardScopedTreeStoreWriter::new(tx, shard);

                for transition in msg.transitions {
                    let transition =
                        StateTransition::try_from(transition).map_err(CommsRpcConsensusSyncError::InvalidResponse)?;
                    if transition.id.shard() != shard {
                        return Err(CommsRpcConsensusSyncError::InvalidResponse(anyhow!(
                            "Received state transition for shard {} which is not the expected shard {}.",
                            transition.id.shard(),
                            shard
                        )));
                    }

                    if transition.id.epoch().is_zero() {
                        return Err(CommsRpcConsensusSyncError::InvalidResponse(anyhow!(
                            "Received state transition with epoch 0."
                        )));
                    }

                    if transition.id.epoch() >= current_epoch {
                        return Err(CommsRpcConsensusSyncError::InvalidResponse(anyhow!(
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
                                            .ok_or_else(|| CommsRpcConsensusSyncError::InvalidResponse(
                                                anyhow!("Validator returned a template address {} but substate value was not a template", id.substate_id())
                                            ))?;

                                        info!(target: LOG_TARGET, "🛜 Add template {id}");
                                        template_changes_mut.push(TemplateChange::Add {
                                            template_address,
                                            author_public_key: template.author.clone(),
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



                    info!(target: LOG_TARGET, "🛜 Applying state update (v{}) {}", current_version.unwrap_or(0), transition);
                    self.commit_update(store.transaction(), checkpoint, transition)?;

                    tree_changes.push(change);
                    if tree_changes.len() == BATCH_SIZE {
                        let mut state_tree = SpreadPrefixStateTree::new(&mut store);
                        info!(target: LOG_TARGET, "🛜 Committing {} state tree changes v{} to v{}", tree_changes.len(), current_version.unwrap_or(0), current_version.unwrap_or(0) + 1);
                        let next_version = current_version.unwrap_or(0) + 1;
                        state_tree.put_substate_changes(current_version, next_version, tree_changes.drain(..))?;
                        current_version = Some(next_version);
                        store.set_version(next_version)?;
                    }
                }

                if !tree_changes.is_empty() {
                    let mut state_tree = SpreadPrefixStateTree::new(&mut store);
                    let next_version = current_version.unwrap_or(0) + 1;
                    info!(target: LOG_TARGET, "🛜 Committing final {} state tree changes v{} to v{}", tree_changes.len(), current_version.unwrap_or(0), next_version);
                    state_tree.put_substate_changes(current_version, next_version, tree_changes.drain(..))?;
                    current_version = Some(next_version);
                    store.set_version(next_version)?;
                }

                Ok::<_, CommsRpcConsensusSyncError>(())
            })?;
        }

        let local_state_root = self
            .state_store
            .with_read_tx(|tx| self.get_state_root_for_shard(tx, shard, current_version))?;
        if local_state_root != checkpoint_state_root {
            error!(
                target: LOG_TARGET,
                "❌State root mismatch for {shard}. Checkpoint {expected} but got {actual}. Rolling back.",
                expected = checkpoint_state_root,
                actual = local_state_root,
            );

            // TODO: rollback
            return Err(CommsRpcConsensusSyncError::StateRootMismatch {
                expected: checkpoint_state_root,
                actual: local_state_root,
            });
        }

        info!(target: LOG_TARGET, "🛜 Synced state for {shard} to v{} with root {local_state_root}", current_version.unwrap_or(0));

        Ok(current_version)
    }

    fn get_state_root_for_shard(
        &self,
        tx: &<TConsensusSpec::StateStore as StateStore>::ReadTransaction<'_>,
        shard: Shard,
        version: Option<Version>,
    ) -> Result<TreeHash, CommsRpcConsensusSyncError> {
        let Some(version) = version else {
            return Ok(SPARSE_MERKLE_PLACEHOLDER_HASH);
        };

        let mut store = ShardScopedTreeStoreReader::new(tx, shard);
        let state_tree = SpreadPrefixStateTree::new(&mut store);
        let root_hash = state_tree.get_root_hash(version)?;
        Ok(root_hash)
    }

    pub fn commit_update<TTx: StateStoreWriteTransaction>(
        &self,
        tx: &mut TTx,
        checkpoint: &EpochCheckpoint,
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
                    NodeHeight(0),
                    *checkpoint.block().id(),
                    substate.created_by_transaction,
                    // TODO: correct QC ID
                    QcId::zero(),
                    // *created_qc.id(),
                )
                .create(tx)?;
            },
            SubstateUpdate::Destroy(SubstateDestroyedProof {
                substate_id,
                version,
                destroyed_by_transaction,
            }) => {
                SubstateRecord::destroy(
                    tx,
                    VersionedSubstateId::new(substate_id, version),
                    transition.id.shard(),
                    transition.id.epoch(),
                    // TODO
                    checkpoint.block().height(),
                    &QcId::zero(),
                    &destroyed_by_transaction,
                )?;
            },
        }

        Ok(())
    }

    async fn get_sync_committees(
        &self,
        current_epoch: Epoch,
    ) -> Result<Vec<(ShardGroup, Committee<PeerAddress>)>, CommsRpcConsensusSyncError> {
        // We are behind at least one epoch.
        // We get the current substate range, and we asks committees from previous epoch in this range to give us
        // data.
        let local_info = self.epoch_manager.get_local_committee_info(current_epoch).await?;
        let prev_epoch = current_epoch.saturating_sub(Epoch(1));
        info!(target: LOG_TARGET,"Previous epoch is {}", prev_epoch);
        // We want to get any committees from the previous epoch that overlap with our shard group in this epoch
        let committees = self
            .epoch_manager
            .get_committees_overlapping_shard_group(prev_epoch, local_info.shard_group())
            .await?;

        if committees.is_empty() {
            return Err(CommsRpcConsensusSyncError::NoCommittees(prev_epoch));
        }

        // not strictly necessary to sort by shard but easier on the eyes in logs
        let mut committees = committees.into_iter().collect::<Vec<_>>();
        committees.sort_by_key(|(k, _)| *k);
        info!(target: LOG_TARGET, "🛜 Querying {} committee(s) from epoch {}", committees.len(), prev_epoch);
        Ok(committees)
    }

    fn validate_checkpoint(&self, checkpoint: &EpochCheckpoint) -> Result<(), CommsRpcConsensusSyncError> {
        // TODO: validate checkpoint

        if !checkpoint.block().is_epoch_end() {
            return Err(CommsRpcConsensusSyncError::InvalidResponse(anyhow!(
                "Checkpoint block is not an Epoch End block"
            )));
        }

        // Sanity check that the calculated merkle root matches the provided shard roots
        // Note this allows us to use each of the provided shard MRs assuming we trust the provided block that has been
        // signed by a BFT majority of registered VNs
        let calculated_root = checkpoint.compute_state_merkle_root()?;
        if calculated_root != *checkpoint.block().state_merkle_root() {
            return Err(CommsRpcConsensusSyncError::InvalidResponse(anyhow!(
                "Checkpoint merkle root mismatch. Expected {expected} but got {actual}",
                expected = checkpoint.block().state_merkle_root(),
                actual = calculated_root,
            )));
        }

        Ok(())
    }

    /// Synchronizes the given [`Shard`].
    pub async fn sync_shard(
        &mut self,
        shard: Shard,
        current_epoch: Epoch,
        committee: &Committee<PeerAddress>,
        our_vn_addr: &PeerAddress,
    ) -> Result<(), CommsRpcConsensusSyncError> {
        let mut remaining_members = committee.len();

        info!(target: LOG_TARGET, "🛜 Syncing state for shard {shard} and epoch {}", current_epoch.saturating_sub(Epoch(1)));
        for addr in committee.addresses() {
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
            let checkpoint = match self.fetch_epoch_checkpoint(&mut client, current_epoch).await {
                Ok(Some(cp)) => cp,
                Ok(None) => {
                    // TODO: we should check with f + 1 validators in this case. If a single validator reports
                    // this falsely, this will prevent us from continuing with consensus for a long time (state
                    // root will mismatch).
                    // TODO: we should instead ask the base layer if this is the first epoch in the network
                    warn!(
                        target: LOG_TARGET,
                        "❓No checkpoint for epoch {current_epoch}. This may mean that this is the first epoch in the network"
                    );
                    return Ok(());
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
            info!(target: LOG_TARGET, "🛜 Checkpoint: {checkpoint}");

            self.validate_checkpoint(&checkpoint)?;
            self.state_store.with_write_tx(|tx| checkpoint.save(tx))?;
            let mut template_changes = vec![];

            match self
                .start_state_sync(&mut client, shard, &checkpoint, &mut template_changes)
                .await
            {
                Ok(_) => {
                    // We only enqueue these if state sync succeeds and the state root matches
                    self.template_manager.enqueue_template_changes(template_changes).await?;
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

            break;
        }

        Ok(())
    }

    async fn sync_global_shard(
        &mut self,
        current_epoch: Epoch,
        committees: &[(ShardGroup, Committee<PeerAddress>)],
        our_vn_address: &PeerAddress,
    ) -> Result<(), CommsRpcConsensusSyncError> {
        let mut last_error = None;

        for (sg, committee) in committees {
            if let Err(err) = self
                .sync_shard(Shard::global(), current_epoch, committee, our_vn_address)
                .await
            {
                warn!(target: LOG_TARGET, "⚠️ Failed to sync global shard from {sg}: {err}. Attempting another committee if available");
                last_error = Some(err);
                continue;
            }
            break;
        }

        if let Some(err) = last_error {
            return Err(err);
        }

        Ok(())
    }
}

impl<TConsensusSpec> SyncManager for RpcStateSyncClientProtocol<TConsensusSpec>
where TConsensusSpec: ConsensusSpec<Addr = PeerAddress> + Send + Sync + 'static
{
    type Error = CommsRpcConsensusSyncError;

    async fn check_sync(&self) -> Result<SyncStatus, Self::Error> {
        let current_epoch = self.epoch_manager.current_epoch().await?;

        let leaf_block = self
            .state_store
            .with_read_tx(|tx| LeafBlock::get(tx, current_epoch).optional())?;

        // We only sync if we're behind by an epoch. The current epoch is replayed in consensus.
        if current_epoch > leaf_block.map_or(Epoch::zero(), |b| b.epoch()) {
            info!(target: LOG_TARGET, "🛜Our current leaf block is behind the current epoch. Syncing...");
            return Ok(SyncStatus::Behind);
        }

        if leaf_block.is_some_and(|l| l.height.is_zero()) {
            // We only have the genesis for the epoch, let's assume we're behind in this case
            info!(target: LOG_TARGET, "🛜Our current leaf block is behind the current epoch. Syncing...");
            return Ok(SyncStatus::Behind);
        }

        Ok(SyncStatus::UpToDate)
    }

    async fn sync(&mut self) -> Result<(), Self::Error> {
        let current_epoch = self.epoch_manager.current_epoch().await?;
        let our_vn = self.epoch_manager.get_our_validator_node(current_epoch).await?;
        let prev_epoch_committees = self.get_sync_committees(current_epoch).await?;

        self.sync_global_shard(current_epoch, &prev_epoch_committees, &our_vn.address)
            .await?;

        // Sync data from each committee in range of the committee we're joining.
        // NOTE: we don't have to worry about substates in address range because shard boundaries are fixed.
        for (shard_group, mut committee) in prev_epoch_committees {
            committee.shuffle();
            for shard in shard_group.shard_iter() {
                self.sync_shard(shard, current_epoch, &committee, &our_vn.address)
                    .await?;
            }
        }

        info!(target: LOG_TARGET, "🛜State sync complete");
        Ok(())
    }
}
