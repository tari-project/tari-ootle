//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use anyhow::anyhow;
use futures::{Stream, StreamExt};
use log::*;
use ootle_network::Network;
use tari_ootle_common_types::{Epoch, ToSubstateAddress, VersionedSubstateId, optional::Optional, shard::Shard};
use tari_ootle_p2p::proto::rpc::{SyncStateResponse, sync_state_response};
use tari_ootle_storage::{
    ShardScopedTreeStoreReader,
    ShardScopedTreeStoreWriter,
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
    consensus_models::{SubstateRecord, SubstateTransition, SubstateUpdateBatch, SubstateUpdateProof},
};
use tari_state_tree::{SPARSE_MERKLE_PLACEHOLDER_HASH, SpreadPrefixStateTree, SubstateTreeChange, TreeHash, Version};
use tari_validator_node_rpc::STATE_SYNC_MAX_BATCH_SIZE;

use crate::{error::RpcStateSyncError, stats::StateSyncStats};

const LOG_TARGET: &str = "tari::ootle::rpc_state_sync::shard_sync";

/// Syncs one shard's state from a peer's stream against the shard root of a trusted checkpoint.
pub(crate) struct ShardSync<'a, TStore> {
    network: Network,
    store: &'a TStore,
    shard: Shard,
    checkpoint_shard_root: TreeHash,
}

impl<'a, TStore: StateStore> ShardSync<'a, TStore> {
    pub fn new(network: Network, store: &'a TStore, shard: Shard, checkpoint_shard_root: TreeHash) -> Self {
        Self {
            network,
            store,
            shard,
            checkpoint_shard_root,
        }
    }

    pub fn local_state_root(&self, version: Option<Version>) -> Result<TreeHash, RpcStateSyncError> {
        self.store
            .with_read_tx(|tx| calculate_state_root_for_shard(tx, self.shard, version))
    }

    /// Rewinds the versions of the shard that an earlier sync committed but never verified against its checkpoint,
    /// and returns the shard's latest verified version.
    pub fn discard_unverified_state(&self) -> Result<Option<Version>, RpcStateSyncError> {
        let shard = self.shard;
        self.store.with_write_tx(|tx| {
            if let Some(rewind_point) = tx.state_sync_rewind_point_get(shard)? {
                let tree_stats = tx.state_tree_truncate_to_version(shard, rewind_point)?;
                let substate_stats = tx.substates_rewind_to_state_version(shard, rewind_point)?;
                tx.state_sync_rewind_point_remove(shard)?;
                warn!(
                    target: LOG_TARGET,
                    "🛜 Discarded unverified synced state for {shard} above v{rewind_point}: {} state version(s), {} \
                     tree node(s)",
                    substate_stats.transitions_processed,
                    tree_stats.nodes_deleted,
                );
            }
            Ok::<_, RpcStateSyncError>(tx.state_tree_versions_get_latest(shard)?)
        })
    }

    /// Syncs the shard from `state_stream` on top of `verified_version`, its latest verified version. Versions are
    /// committed as they arrive and discarded again unless the stream completes with the shard matching the
    /// checkpoint root, so only verified state outlives a sync.
    pub async fn sync_from_stream<S, E>(
        &self,
        stats: &mut StateSyncStats,
        verified_version: Option<Version>,
        state_stream: S,
    ) -> Result<Option<Version>, RpcStateSyncError>
    where
        S: Stream<Item = Result<SyncStateResponse, E>> + Unpin,
        RpcStateSyncError: From<E>,
    {
        let result = self.apply_stream(stats, verified_version, state_stream).await;
        if result.is_err() &&
            let Err(err) = self.discard_unverified_state()
        {
            // The rewind point is still stored, so the next sync attempt discards the state first.
            error!(
                target: LOG_TARGET,
                "❌ Failed to discard unverified synced state for {}: {err}",
                self.shard,
            );
        }
        result
    }

    #[allow(clippy::too_many_lines)]
    async fn apply_stream<S, E>(
        &self,
        stats: &mut StateSyncStats,
        mut maybe_persisted_state_version: Option<Version>,
        mut state_stream: S,
    ) -> Result<Option<Version>, RpcStateSyncError>
    where
        S: Stream<Item = Result<SyncStateResponse, E>> + Unpin,
        RpcStateSyncError: From<E>,
    {
        let shard = self.shard;
        let rewind_point = maybe_persisted_state_version.unwrap_or(0);
        let mut has_unverified_state = false;
        let start_state_version = maybe_persisted_state_version.map_or(1, |v| v + 1);
        let mut last_state_version = start_state_version;
        let mut tree_changes = vec![];
        let mut updates = vec![];
        let mut expected_state_version = None;

        // syncing states
        while let Some(result) = state_stream.next().await {
            let msg = result?;
            let batch = match msg.response {
                Some(sync_state_response::Response::Batch(batch)) => batch,
                Some(sync_state_response::Response::Complete(complete)) => {
                    if complete.shard != shard.as_u32() {
                        return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                            "Received completion marker for shard {} but requested {shard}",
                            complete.shard,
                        )));
                    }
                    // The stream always terminates with a completion marker. Verify the synced shard
                    // root against the trusted checkpoint at our last committed version: the producer
                    // streamed every transition up to the checkpoint epoch, so any gap to
                    // checkpoint_state_version is tree-only (no substate change) and the root at our
                    // last written version equals the checkpoint root. The marker's own version is the
                    // producer's claim and is not trusted as the verification target.
                    debug!(
                        target: LOG_TARGET,
                        "🛜 Stream complete for {shard} (peer reported v{}, locally committed v{})",
                        complete.synced_to_version,
                        maybe_persisted_state_version.unwrap_or(0),
                    );
                    let local_state_root = self.local_state_root(maybe_persisted_state_version)?;
                    if local_state_root != self.checkpoint_shard_root {
                        error!(
                            target: LOG_TARGET,
                            "❌ State root mismatch for {shard}. Checkpoint {expected} but got {actual}.",
                            expected = self.checkpoint_shard_root,
                            actual = local_state_root,
                        );
                        return Err(RpcStateSyncError::StateRootMismatch {
                            expected: self.checkpoint_shard_root,
                            actual: local_state_root,
                        });
                    }
                    if has_unverified_state {
                        self.store
                            .with_write_tx(|tx| tx.state_sync_rewind_point_remove(shard))?;
                    }
                    info!(
                        target: LOG_TARGET,
                        "🛜 ✅ State root for {shard} matches checkpoint: {local_state_root} (v{})",
                        maybe_persisted_state_version.unwrap_or(0),
                    );
                    return Ok(maybe_persisted_state_version);
                },
                None => {
                    return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                        "Received sync state response with no variant set."
                    )));
                },
            };

            if batch.shard != shard.as_u32() {
                return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                    "Received batch for shard {} but requested {shard}",
                    batch.shard,
                )));
            }
            if batch.updates.is_empty() {
                return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                    "Received empty state transition batch."
                )));
            }
            if batch.updates.len() > STATE_SYNC_MAX_BATCH_SIZE {
                return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                    "Received too many state updates in a batch: {}. Expected at most {}.",
                    batch.updates.len(),
                    STATE_SYNC_MAX_BATCH_SIZE
                )));
            }
            if batch.state_version < start_state_version {
                return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                    "Received state version {} that is less than the persisted state version {}.",
                    batch.state_version,
                    start_state_version
                )));
            }

            if expected_state_version.is_some_and(|v| v != batch.state_version) {
                return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                    "Received state version {} that is not the expected state version {}.",
                    batch.state_version,
                    expected_state_version.unwrap()
                )));
            }

            let state_version = batch.state_version;
            if state_version < last_state_version {
                return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                    "Received state version {} that is less than the last state version {}.",
                    state_version,
                    last_state_version
                )));
            }

            last_state_version = state_version;

            stats.total_transitions += batch.updates.len() as u64;

            tree_changes.reserve_exact(batch.updates.len());
            updates.reserve_exact(batch.updates.len());

            let updates_for_state_version = batch
                .updates
                .into_iter()
                .map(|t| SubstateUpdateProof::try_from(t).map_err(RpcStateSyncError::InvalidResponse));
            let msg_epoch = batch.epoch.map(Epoch::from).ok_or_else(|| {
                RpcStateSyncError::InvalidResponse(anyhow!("Received state transition with no epoch"))
            })?;

            info!(target: LOG_TARGET, "🛜 Buffering {} state update(s) (state version: v{})", updates_for_state_version.len(), state_version);
            for result in updates_for_state_version {
                let update = result?;
                let tree_change = extract_tree_change(self.network, &update, msg_epoch);

                debug!(target: LOG_TARGET, "🛜 -> state update (v{}) {}", state_version, update);
                tree_changes.push(tree_change);
                updates.push(update);
            }

            info!(target: LOG_TARGET, "🛜 Sync: {} state update(s), state version: v{}", updates.len(), state_version);

            if batch.has_more {
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

            // Commit the buffered changes for this state version. The shard root is verified once, on
            // the terminal SyncComplete, against the trusted checkpoint. Until then the rewind point
            // marks every version committed above it as unverified, including across a restart.
            self.store.with_write_tx(|tx| {
                info!(
                    target: LOG_TARGET,
                    "🛜 Next state updates batch of size {} from v{}",
                    updates.len(),
                    state_version
                );

                check_updates_apply(&**tx, state_version, &updates)?;
                if !has_unverified_state {
                    tx.state_sync_rewind_point_set(shard, rewind_point)?;
                }

                let mut store = ShardScopedTreeStoreWriter::new(tx, shard);

                info!(target: LOG_TARGET, "🛜 {} state update(s) for v{}", updates.len(), state_version);
                commit_updates(
                    self.network,
                    store.transaction(),
                    shard,
                    msg_epoch,
                    state_version,
                    updates.drain(..),
                )?;

                // Persist tree changes
                if !tree_changes.is_empty() {
                    let mut state_tree = SpreadPrefixStateTree::new(&mut store);
                    info!(target: LOG_TARGET, "🛜 Committing {} state tree changes batch v{}", tree_changes.len(), state_version);
                    state_tree.batch_put_substate_changes(maybe_persisted_state_version, state_version, tree_changes.drain(..))?;
                    maybe_persisted_state_version = Some(state_version);
                    store.set_state_version(state_version)?;
                }

                Ok::<_, RpcStateSyncError>(())
            })?;
            has_unverified_state = true;
        }

        // The stream ended without a SyncComplete - the peer closed early, so the sync is unverified.
        Err(RpcStateSyncError::InvalidResponse(anyhow!(
            "State sync stream for {shard} ended without a completion marker"
        )))
    }
}

/// Rejects a state version with a transition that does not apply cleanly to local state: creating a substate that
/// already exists, or destroying one that is not up. A rewind inverts each committed transition, which restores the
/// prior state only if every transition applied cleanly.
fn check_updates_apply<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    state_version: Version,
    updates: &[SubstateUpdateProof],
) -> Result<(), RpcStateSyncError> {
    let mut created = HashSet::new();
    let mut destroyed = HashSet::new();
    for update in updates {
        let id = update.to_versioned_substate_id();
        let address = id.to_substate_address();
        match update {
            SubstateUpdateProof::Create(_) => {
                if !created.insert(address) || tx.substates_get(&address).optional()?.is_some() {
                    return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                        "Peer streamed the creation of {id} at v{state_version}, but {id} already exists"
                    )));
                }
            },
            SubstateUpdateProof::Destroy(_) => {
                let is_up =
                    created.contains(&address) || tx.substates_get(&address).optional()?.is_some_and(|s| s.is_up());
                if !is_up || !destroyed.insert(address) {
                    return Err(RpcStateSyncError::InvalidResponse(anyhow!(
                        "Peer streamed the destruction of {id} at v{state_version}, but {id} is not up"
                    )));
                }
            },
        }
    }
    Ok(())
}

pub(crate) fn calculate_state_root_for_shard<TTx: StateStoreReadTransaction>(
    tx: &TTx,
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

fn commit_updates<TTx: StateStoreWriteTransaction, I: IntoIterator<Item = SubstateUpdateProof>>(
    network: Network,
    tx: &mut TTx,
    shard: Shard,
    epoch: Epoch,
    state_version: Version,
    updates: I,
) -> Result<(), StorageError> {
    let mut batch = SubstateUpdateBatch::new(network, epoch);

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

fn extract_tree_change(network: Network, update: &SubstateUpdateProof, epoch: Epoch) -> SubstateTreeChange {
    match update {
        SubstateUpdateProof::Create(create) => {
            let id = create.substate.as_versioned_substate_id_ref();
            SubstateTreeChange::Up {
                id: id.to_owned(),
                value_hash: create.substate.to_value_hash(network, epoch),
            }
        },
        SubstateUpdateProof::Destroy(destroy) => SubstateTreeChange::Down {
            id: destroy.to_versioned_substate_id(),
        },
    }
}

#[cfg(test)]
mod tests {
    use futures::{FutureExt, stream};
    use tari_engine_types::substate::SubstateId;
    use tari_ootle_common_types::{SubstateAddress, SubstateVersion, optional::Optional};
    use tari_ootle_p2p::proto::rpc::{SubstateBatch, SyncComplete};
    use tari_ootle_storage::consensus_models::{SubstateCreate, SubstateData, SubstateDestroy, SubstateValueOrHash};
    use tari_rpc_framework::RpcStatus;
    use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};
    use tari_state_tree::memory_store::MemoryTreeStore;
    use tari_template_lib_types::{ComponentAddress, Hash32, ObjectKey};
    use tempfile::TempDir;

    use super::*;

    const NETWORK: Network = Network::LocalNet;
    const EPOCH: Epoch = Epoch(1);
    const HONEST: u8 = 1;
    const POISON: u8 = 0xBA;

    type Versions = Vec<(Version, Vec<SubstateUpdateProof>)>;

    fn shard() -> Shard {
        Shard::from(3u32)
    }

    fn create_store() -> (RocksDbStateStore<String>, TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let store = RocksDbStateStore::open(tmp.path().join("rocksdb"), DatabaseOptions::default()).unwrap();
        (store, tmp)
    }

    fn substate_id(seed: u8) -> SubstateId {
        SubstateId::Component(ComponentAddress::from_array([seed; ObjectKey::LENGTH]))
    }

    fn address(seed: u8) -> SubstateAddress {
        SubstateAddress::from_substate_id(&substate_id(seed), SubstateVersion::ZERO)
    }

    fn create(seed: u8) -> SubstateUpdateProof {
        create_with_value(seed, seed)
    }

    fn create_with_value(seed: u8, value: u8) -> SubstateUpdateProof {
        SubstateUpdateProof::Create(Box::new(SubstateCreate {
            substate: SubstateData {
                substate_id: substate_id(seed),
                version: SubstateVersion::ZERO,
                value: SubstateValueOrHash::Hash(Hash32::from_array([value; 32])),
                template_metadata: None,
            },
        }))
    }

    fn destroy(seed: u8) -> SubstateUpdateProof {
        SubstateUpdateProof::Destroy(SubstateDestroy {
            substate_id: substate_id(seed),
            version: SubstateVersion::ZERO,
        })
    }

    fn batch(state_version: Version, updates: Vec<SubstateUpdateProof>) -> Result<SyncStateResponse, RpcStatus> {
        Ok(SyncStateResponse {
            response: Some(sync_state_response::Response::Batch(SubstateBatch {
                state_version,
                updates: updates.into_iter().map(Into::into).collect(),
                has_more: false,
                epoch: Some(EPOCH.into()),
                shard: shard().as_u32(),
            })),
        })
    }

    fn stream_of(versions: &Versions) -> Vec<Result<SyncStateResponse, RpcStatus>> {
        versions
            .iter()
            .map(|(version, updates)| batch(*version, updates.clone()))
            .collect()
    }

    fn complete(synced_to_version: Version) -> Result<SyncStateResponse, RpcStatus> {
        Ok(SyncStateResponse {
            response: Some(sync_state_response::Response::Complete(SyncComplete {
                synced_to_version,
                epoch: Some(EPOCH.into()),
                shard: shard().as_u32(),
                is_final: true,
            })),
        })
    }

    /// The shard root a checkpoint commits to once `versions` are applied in order.
    fn root_after(versions: &Versions) -> TreeHash {
        let mut store = MemoryTreeStore::new();
        let mut tree = SpreadPrefixStateTree::new(&mut store);
        let mut prev = None;
        let mut root = SPARSE_MERKLE_PLACEHOLDER_HASH;
        for (version, updates) in versions {
            root = tree
                .put_substate_changes(
                    prev,
                    *version,
                    updates.iter().map(|u| extract_tree_change(NETWORK, u, EPOCH)),
                )
                .unwrap();
            prev = Some(*version);
        }
        root
    }

    fn local_version<TStore: StateStore>(store: &TStore) -> Option<Version> {
        store
            .with_read_tx(|tx| tx.state_tree_versions_get_latest(shard()))
            .unwrap()
    }

    fn rewind_point<TStore: StateStore>(store: &TStore) -> Option<Version> {
        store
            .with_read_tx(|tx| tx.state_sync_rewind_point_get(shard()))
            .unwrap()
    }

    fn substate<TStore: StateStore>(store: &TStore, seed: u8) -> Option<SubstateRecord> {
        store
            .with_read_tx(|tx| tx.substates_get(&address(seed)).optional())
            .unwrap()
    }

    async fn sync<TStore: StateStore>(
        store: &TStore,
        checkpoint: &Versions,
        responses: Vec<Result<SyncStateResponse, RpcStatus>>,
    ) -> Result<Option<Version>, RpcStateSyncError> {
        let sync = ShardSync::new(NETWORK, store, shard(), root_after(checkpoint));
        let verified_version = sync.discard_unverified_state()?;
        sync.sync_from_stream(
            &mut StateSyncStats::default(),
            verified_version,
            stream::iter(responses),
        )
        .await
    }

    /// Syncs `versions` from an honest peer, leaving them as the store's verified state.
    async fn sync_honestly<TStore: StateStore>(store: &TStore, versions: &Versions) {
        let mut responses = stream_of(versions);
        responses.push(complete(versions.last().unwrap().0));
        sync(store, versions, responses).await.unwrap();
    }

    #[tokio::test]
    async fn a_stream_that_matches_the_checkpoint_is_kept() {
        let (store, _tmp) = create_store();
        let honest = vec![(1, vec![create(HONEST)]), (2, vec![create(2)])];

        sync_honestly(&store, &honest).await;

        assert_eq!(local_version(&store), Some(2));
        assert!(substate(&store, HONEST).is_some_and(|s| s.is_up()));
        assert!(substate(&store, 2).is_some_and(|s| s.is_up()));
        assert_eq!(rewind_point(&store), None);
    }

    #[tokio::test]
    async fn a_stream_that_closes_early_leaves_no_state_behind() {
        let (store, _tmp) = create_store();
        let honest = vec![(1, vec![create(HONEST)])];

        let err = sync(&store, &honest, vec![batch(1, vec![create(HONEST), create(POISON)])])
            .await
            .unwrap_err();

        assert!(matches!(err, RpcStateSyncError::InvalidResponse(_)), "{err}");
        assert_eq!(local_version(&store), None);
        assert!(substate(&store, POISON).is_none());
        assert!(substate(&store, HONEST).is_none());
        assert_eq!(rewind_point(&store), None);
    }

    #[tokio::test]
    async fn a_stream_that_misses_the_checkpoint_root_leaves_no_state_behind() {
        let (store, _tmp) = create_store();
        let honest = vec![(1, vec![create(HONEST)])];

        let err = sync(&store, &honest, vec![
            batch(1, vec![create(HONEST), create(POISON)]),
            complete(1),
        ])
        .await
        .unwrap_err();

        assert!(matches!(err, RpcStateSyncError::StateRootMismatch { .. }), "{err}");
        assert_eq!(local_version(&store), None);
        assert!(substate(&store, POISON).is_none());
        assert_eq!(rewind_point(&store), None);
    }

    #[tokio::test]
    async fn a_failed_sync_restores_the_verified_state_it_started_from() {
        let (store, _tmp) = create_store();
        let verified = vec![(1, vec![create(HONEST)])];
        sync_honestly(&store, &verified).await;

        let mut honest = verified.clone();
        honest.push((2, vec![create(2)]));
        sync(&store, &honest, vec![batch(2, vec![destroy(HONEST), create(POISON)])])
            .await
            .unwrap_err();

        assert_eq!(local_version(&store), Some(1));
        assert!(substate(&store, HONEST).is_some_and(|s| s.is_up()));
        assert!(substate(&store, POISON).is_none());
        let shard_sync = ShardSync::new(NETWORK, &store, shard(), root_after(&verified));
        assert_eq!(shard_sync.local_state_root(Some(1)).unwrap(), root_after(&verified));

        sync(&store, &honest, vec![batch(2, vec![create(2)]), complete(2)])
            .await
            .unwrap();
        assert_eq!(local_version(&store), Some(2));
    }

    #[tokio::test]
    async fn a_version_that_overwrites_local_state_is_rejected() {
        let (store, _tmp) = create_store();
        let verified = vec![(1, vec![create(HONEST)])];
        sync_honestly(&store, &verified).await;

        let err = sync(&store, &verified, vec![batch(2, vec![create_with_value(
            HONEST, POISON,
        )])])
        .await
        .unwrap_err();
        assert!(matches!(err, RpcStateSyncError::InvalidResponse(_)), "{err}");

        let err = sync(&store, &verified, vec![batch(2, vec![
            destroy(HONEST),
            destroy(HONEST),
        ])])
        .await
        .unwrap_err();
        assert!(matches!(err, RpcStateSyncError::InvalidResponse(_)), "{err}");

        let record = substate(&store, HONEST).unwrap();
        assert!(record.is_up());
        assert_eq!(*record.state_hash(), Hash32::from_array([HONEST; 32]));
        assert_eq!(local_version(&store), Some(1));
    }

    #[tokio::test]
    async fn an_interrupted_sync_is_discarded_before_the_next_attempt() {
        let (store, _tmp) = create_store();
        let honest = vec![(1, vec![create(HONEST)])];
        let sync = ShardSync::new(NETWORK, &store, shard(), root_after(&honest));

        // The peer stalls after one version and the sync is dropped mid-stream, as on shutdown.
        let stalled = stream::iter(vec![batch(1, vec![create(POISON)])]).chain(stream::pending());
        let interrupted = sync
            .sync_from_stream(&mut StateSyncStats::default(), None, stalled)
            .now_or_never();
        assert!(interrupted.is_none());
        assert!(substate(&store, POISON).is_some());
        assert_eq!(rewind_point(&store), Some(0));

        assert_eq!(sync.discard_unverified_state().unwrap(), None);
        assert_eq!(local_version(&store), None);
        assert!(substate(&store, POISON).is_none());
        assert_eq!(rewind_point(&store), None);
    }
}
