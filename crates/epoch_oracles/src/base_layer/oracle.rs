//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::VecDeque,
    future::{Future, poll_fn},
    pin::Pin,
    task::{Context, Poll},
};

use log::*;
use ootle_network::Network;
use tari_base_node_client::{
    BaseNodeClient,
    BaseNodeClientError,
    futures_util::TryStreamExt,
    grpc::GrpcBaseNodeClient,
    types::{BaseLayerConsensusConstants, BaseLayerMetadata},
};
use tari_common_types::types::FixedHash;
use tari_epoch_manager::epoch_event_oracle::{EpochEvent, EpochEventOracle};
use tari_node_components::blocks::BlockHeader;
use tari_ootle_common_types::{Epoch, displayable::Displayable, optional::Optional};
use tokio::time;

use crate::{
    base_layer::{BaseLayerBlockHeaderStore, BaseLayerEpochOracleConfig, header_hasher::hash_header},
    store::{EpochOracleStore, StoreKey},
};

const LOG_TARGET: &str = "tari::ootle::epoch_oracles::base_layer_scanner";

type TaskOutput<TStore, TClient> = (
    Result<bool, BaseLayerOracleError>,
    Box<BaseLayerOracleInner<TStore, TClient>>,
);

/// The in-flight blockchain scan future, boxed so it can be held across poll calls.
type ScanTask<TStore, TClient> = Pin<Box<dyn Future<Output = TaskOutput<TStore, TClient>> + Send>>;

/// `TClient` defaults to [`GrpcBaseNodeClient`] for production; tests substitute a mock base node
/// client so the scan/reorg logic can be driven without a live base node.
#[allow(clippy::struct_excessive_bools)]
pub struct BaseLayerOracle<TStore, TClient = GrpcBaseNodeClient> {
    inner: Option<Box<BaseLayerOracleInner<TStore, TClient>>>,
    is_initialized: bool,
    is_done: bool,
    has_more: bool,
    sleep_or_shutdown: bool,
    task: Option<ScanTask<TStore, TClient>>,
    sleep_task: Option<Pin<Box<time::Sleep>>>,
}

struct BaseLayerOracleInner<TStore, TClient> {
    config: BaseLayerEpochOracleConfig,
    store: TStore,
    last_scanned_height: u64,
    last_scanned_tip: Option<FixedHash>,
    last_scanned_hash: Option<FixedHash>,
    last_epoch_hash: Option<FixedHash>,
    last_scanned_validator_node_mr: Option<FixedHash>,
    base_node_client: TClient,
    has_attempted_scan: bool,
    pending_events: VecDeque<EpochEvent>,
    header_buf: Vec<(Epoch, FixedHash, BlockHeader)>,
    network: Network,
    /// Epoch length in base-layer blocks, cached from consensus constants on the first scan
    /// that obtains them. `None` until the first scan completes.
    cached_epoch_length: Option<u64>,
}

impl<TStore: EpochOracleStore + BaseLayerBlockHeaderStore + 'static, TClient: BaseNodeClient>
    BaseLayerOracle<TStore, TClient>
{
    pub fn new(store: TStore, base_node_client: TClient, config: BaseLayerEpochOracleConfig, network: Network) -> Self {
        Self {
            inner: Some(Box::new(BaseLayerOracleInner {
                config,
                store,
                last_scanned_tip: None,
                last_scanned_height: 0,
                last_scanned_hash: None,
                last_epoch_hash: None,
                last_scanned_validator_node_mr: None,
                base_node_client,
                has_attempted_scan: false,
                pending_events: VecDeque::new(),
                header_buf: Vec::new(),
                network,
                cached_epoch_length: None,
            })),
            is_initialized: false,
            is_done: false,
            has_more: false,
            sleep_or_shutdown: false,
            task: None,
            sleep_task: None,
        }
    }
}

impl<TStore: EpochOracleStore + BaseLayerBlockHeaderStore, TClient: BaseNodeClient>
    BaseLayerOracleInner<TStore, TClient>
{
    /// The configured start height adjusted for the height lag. This is the minimum height
    /// that the scanner will scan from.
    fn lag_start_height(&self) -> u64 {
        self.config.start_height.saturating_sub(self.config.height_lag)
    }

    /// The effective last scanned height, which is the maximum of the last scanned height and
    /// the lag start height. This ensures that we never scan below the lag start height.
    fn effective_last_scanned_height(&self) -> u64 {
        self.last_scanned_height.max(self.lag_start_height())
    }

    fn load_initial_state(&mut self) -> Result<(), BaseLayerOracleError> {
        self.last_scanned_tip = self
            .store
            .get(StoreKey::BaseLayerLastScannedTip.as_key_bytes())
            .map_err(BaseLayerOracleError::StoreError)?;
        self.last_scanned_hash = self
            .store
            .get(StoreKey::BaseLayerLastScannedBlockHash.as_key_bytes())
            .map_err(BaseLayerOracleError::StoreError)?;
        self.last_epoch_hash = self
            .store
            .get(StoreKey::BaseLayerLastEpochHash.as_key_bytes())
            .map_err(BaseLayerOracleError::StoreError)?;
        self.last_scanned_height = self
            .store
            .get(StoreKey::BaseLayerLastScannedBlockHeight.as_key_bytes())
            .map_err(BaseLayerOracleError::StoreError)?
            .unwrap_or(0);
        Ok(())
    }

    async fn scan_blockchain(&mut self, force_sync: bool) -> Result<bool, BaseLayerOracleError> {
        // fetch the new base layer info since the previous scan
        let tip = self.base_node_client.get_tip_info().await?;
        if force_sync {
            info!(
                target: LOG_TARGET,
                "⛓️ Forcing blockchain sync. We last scanned {}/{}",
                self.last_scanned_height,
                tip.height_of_longest_chain
                    .saturating_sub(self.config.height_lag)
            );
            self.has_attempted_scan = true;
            return self.sync_blockchain(tip).await;
        }

        match self.get_blockchain_progression(&tip).await? {
            BlockchainProgression::Progressed => {
                info!(
                    target: LOG_TARGET,
                    "⛓️ Blockchain has progressed to height {}. We last scanned {}/{}",
                    tip.height_of_longest_chain,
                    self.effective_last_scanned_height(),
                    tip.height_of_longest_chain
                        .saturating_sub(self.config.height_lag)
                );
                self.has_attempted_scan = true;
                self.sync_blockchain(tip).await
            },
            BlockchainProgression::Reorged => {
                warn!(
                    target: LOG_TARGET,
                    "⚠️ Base layer reorg detected at scanned height {}. Locating fork point.",
                    self.last_scanned_height
                );
                self.handle_reorg(&tip).await?;
                self.has_attempted_scan = true;
                self.sync_blockchain(tip).await
            },
            BlockchainProgression::NoProgress => {
                trace!(target: LOG_TARGET, "No new blocks to scan.");
                if !self.has_attempted_scan {
                    let constants = self
                        .base_node_client
                        .get_consensus_constants(tip.height_of_longest_chain)
                        .await?;
                    self.cached_epoch_length = Some(constants.epoch_length());
                    let lagged_height = tip.height_of_longest_chain.saturating_sub(self.config.height_lag);
                    let epoch = constants.height_to_epoch(lagged_height);
                    // If no progress has been made since restarting, we still need to tell the epoch manager that
                    // scanning is done
                    self.pending_events.push_back(EpochEvent::DoneForNow {
                        epoch,
                        epoch_hash: self.last_epoch_hash.unwrap_or_else(FixedHash::zero),
                    });
                }

                self.has_attempted_scan = true;
                Ok(false)
            },
        }
    }

    async fn get_blockchain_progression(
        &mut self,
        tip: &BaseLayerMetadata,
    ) -> Result<BlockchainProgression, BaseLayerOracleError> {
        if tip.height_of_longest_chain <= self.lag_start_height() {
            return Ok(BlockchainProgression::NoProgress);
        }
        // Short-circuit when the un-lagged tip is unchanged — nothing to do.
        if self.last_scanned_tip.as_ref() == Some(&tip.tip_hash) {
            return Ok(BlockchainProgression::NoProgress);
        }
        // Probe for reorgs using our deepest scanned block, NOT the un-lagged tip. The un-lagged
        // tip naturally gets displaced every block or two when a competing miner's tip is
        // orphaned — that lookup returns NotFound and declares "reorg" even though nothing at our
        // (lagged) scan depth has changed. Using last_scanned_hash restricts reorg detection to
        // changes that actually invalidate data we've stored.
        match &self.last_scanned_hash {
            Some(hash) => {
                let header = self.base_node_client.get_header_by_hash(hash).await.optional()?;
                if header.is_some() {
                    Ok(BlockchainProgression::Progressed)
                } else {
                    Ok(BlockchainProgression::Reorged)
                }
            },
            None => Ok(BlockchainProgression::Progressed),
        }
    }

    /// Recovers from a detected base-layer reorg by rewinding the scanner to the fork point.
    ///
    /// Walks backwards from the deepest height we scanned, asking the base node for the block on the
    /// (new) canonical chain at each height. The fork point is the highest such height whose canonical
    /// block we have already stored — everything above it was built on the orphaned chain. We delete
    /// those stale headers and rewind our scan position to the fork point, so the subsequent
    /// `sync_blockchain` re-fetches and stores the canonical headers in their place. The epoch database
    /// therefore converges on the correct chain rather than accumulating orphaned headers.
    ///
    /// We only attempt surgical recovery for reorgs within the confirmation depth (`height_lag`); a reorg
    /// deeper than that is treated as unrecoverable — we discard all stored headers and rescan from the
    /// start height. This both matches the confirmation-depth guarantee (boundaries are only emitted once
    /// `height_lag`-buried) and bounds the backward probe below to at most `height_lag` base-node calls.
    async fn handle_reorg(&mut self, tip: &BaseLayerMetadata) -> Result<(), BaseLayerOracleError> {
        let floor = self.lag_start_height();

        // Without persisted headers there is nothing in the epoch DB to repair; rewind to the start of
        // our range and let the normal scan re-emit events on the new chain.
        if !self.config.features.sync_headers {
            self.rewind_to_floor();
            return Ok(());
        }

        let constants = self
            .base_node_client
            .get_consensus_constants(tip.height_of_longest_chain)
            .await?;
        // Bound the lookup epoch by the tip epoch (never excludes a stored header, even if the L1
        // epoch_length constant changed since the header was stored).
        let max_epoch = constants.height_to_epoch(tip.height_of_longest_chain);

        // The fork point cannot be above the new canonical tip, nor above the deepest height we scanned.
        // We only walk back `height_lag` blocks: a deeper fork is treated as unrecoverable below.
        let mut height = self.last_scanned_height.min(tip.height_of_longest_chain);
        let search_floor = self
            .last_scanned_height
            .saturating_sub(self.config.height_lag)
            .max(floor);
        while height > search_floor {
            let epoch = constants.height_to_epoch(height);
            // The block on the new canonical chain at this height (None if the chain is now shorter).
            let Some(canonical) = self.fetch_canonical_header_at(height).await? else {
                height -= 1;
                continue;
            };
            let canonical_hash = hash_header(self.network, &canonical);
            if self
                .store
                .find_block_header_by_hash(max_epoch, &canonical_hash)
                .map_err(BaseLayerOracleError::StoreError)?
                .is_some()
            {
                let num_deleted = self
                    .store
                    .delete_block_headers_above(height)
                    .map_err(BaseLayerOracleError::StoreError)?;
                info!(
                    target: LOG_TARGET,
                    "⚓ Base layer fork point at height {height} ({canonical_hash}). Deleted {num_deleted} stale \
                     header(s) above it; rewinding scanner."
                );
                self.last_scanned_height = height;
                self.last_scanned_hash = Some(canonical_hash);
                self.last_scanned_validator_node_mr = Some(canonical.validator_node_mr);
                self.reset_last_epoch_hash_to_boundary(epoch, &constants)?;
                return Ok(());
            }
            height -= 1;
        }

        // No common ancestor within the recoverable range: discard everything and rebuild from the start
        // height. The re-scan re-crosses (and re-emits) every epoch boundary above the floor, so the epoch
        // hashes self-correct.
        let num_deleted = self
            .store
            .delete_block_headers_above(floor)
            .map_err(BaseLayerOracleError::StoreError)?;
        warn!(
            target: LOG_TARGET,
            "⚠️ Base layer reorg deeper than the recoverable range (start height {floor}). Deleted {num_deleted} \
             header(s); rescanning from the start."
        );
        self.rewind_to_floor();
        Ok(())
    }

    /// After rewinding to a fork point in `fork_epoch`, points `last_epoch_hash` back at the boundary block
    /// that opened that epoch. Boundaries at or below the fork point are canonical, so this discards any
    /// orphaned boundary hash carried over from a chain-shortening reorg — the re-scan resumes *above* the
    /// fork point and so never re-crosses the fork epoch's own boundary to correct it.
    ///
    /// If that boundary block sits below our scan range (so we never emitted it), there is no stored hash
    /// to restore. We clear `last_epoch_hash` rather than leave it: in that case it can only hold a
    /// boundary from an epoch *above* the fork epoch (the fork epoch's own boundary is below the floor and
    /// was never crossed), which a chain-shortening reorg has orphaned. `None` is the same "unknown"
    /// sentinel used at startup, and the re-scan re-populates it on the next boundary it crosses.
    fn reset_last_epoch_hash_to_boundary(
        &mut self,
        fork_epoch: Epoch,
        constants: &BaseLayerConsensusConstants,
    ) -> Result<(), BaseLayerOracleError> {
        let boundary_height = fork_epoch.as_u64().saturating_mul(constants.epoch_length());
        match self
            .store
            .get_first_block_header_in_epoch(fork_epoch)
            .map_err(BaseLayerOracleError::StoreError)?
        {
            Some(boundary) if boundary.height == boundary_height => {
                self.last_epoch_hash = Some(boundary.block_hash);
            },
            _ => {
                debug!(
                    target: LOG_TARGET,
                    "Fork epoch {fork_epoch} boundary block is not within the scan range; clearing last_epoch_hash"
                );
                self.last_epoch_hash = None;
            },
        }
        Ok(())
    }

    /// Fetches the block header on the canonical chain at the given height, or `None` if the chain does
    /// not (yet) extend to it.
    async fn fetch_canonical_header_at(&mut self, height: u64) -> Result<Option<BlockHeader>, BaseLayerOracleError> {
        let mut stream = self.base_node_client.stream_headers(height, 1).await?;
        Ok(stream.try_next().await?)
    }

    /// Resets the in-memory scan position to the start of our scan range, as on a fresh start. The
    /// persisted position is corrected by the next successful `set_last_scanned_block`. `last_epoch_hash`
    /// is cleared too so a reorg that orphaned the last boundary we crossed does not leave a stale hash;
    /// the re-scan re-emits every boundary above the floor and re-populates it.
    fn rewind_to_floor(&mut self) {
        self.last_scanned_height = self.lag_start_height();
        self.last_scanned_hash = None;
        self.last_scanned_validator_node_mr = None;
        self.last_epoch_hash = None;
    }

    #[allow(clippy::too_many_lines)]
    async fn sync_blockchain(&mut self, tip: BaseLayerMetadata) -> Result<bool, BaseLayerOracleError> {
        let Some(lag_tip_height) = tip.height_of_longest_chain.checked_sub(self.config.height_lag) else {
            debug!(
                target: LOG_TARGET,
                "Base layer blockchain is not yet at the required height to start scanning it"
            );
            return Ok(false);
        };
        let start_scan_height = self.effective_last_scanned_height() + 1;
        if lag_tip_height < start_scan_height {
            info!(
                target: LOG_TARGET,
                "⛓️ Base layer blockchain has not progressed beyond the start scan height {}. Lagged tip height is {}.",
                start_scan_height,
                lag_tip_height
            );
            return Ok(false);
        }

        let Some(num_blocks) = lag_tip_height.checked_sub(start_scan_height - 1) else {
            debug!(
                target: LOG_TARGET,
                "Base layer blockchain has not progressed beyond the last scanned height {} (lagged tip height \
                 {}).",
                start_scan_height,
                lag_tip_height
            );
            return Ok(false);
        };

        // Recover the last scanned validator node MR if it is not set yet, i.e the node has scanned BL blocks
        // previously.
        if self.last_scanned_validator_node_mr.is_none() &&
            let Some(ref hash) = self.last_scanned_hash
        {
            let header = self.base_node_client.get_header_by_hash(hash).await?;
            self.last_scanned_validator_node_mr = Some(header.validator_node_mr);
            // cached_header = Some(header);
        }

        let constants = self
            .base_node_client
            .get_consensus_constants(tip.height_of_longest_chain)
            .await?;
        self.cached_epoch_length = Some(constants.epoch_length());

        // We'll buffer 1000 headers at a time
        // note: 10_000 is the maximum permitted by the base node gRPC service
        let limit = 1_000.min(num_blocks);

        let mut base_node_client = self.base_node_client.clone();
        info!(
            target: LOG_TARGET,
            "⛓️Starting header stream from {}-{}", start_scan_height, start_scan_height + limit
        );
        let mut stream = base_node_client.stream_headers(start_scan_height, limit).await?;

        if let Some(additional) = usize::try_from(limit)
            .ok()
            .and_then(|l| l.checked_sub(self.header_buf.capacity()))
        {
            self.header_buf.reserve(additional);
        }

        let mut scan_epoch = constants.height_to_epoch(start_scan_height);
        while let Some(header) = stream.try_next().await? {
            let current_epoch = constants.height_to_epoch(header.height);
            let header_height = header.height;
            // Note: Cant use header.hash() because it uses CURRENT_NETWORK global
            let header_hash = hash_header(self.network, &header);
            let current_validator_node_mr = header.validator_node_mr;
            if self.config.features.sync_headers {
                self.header_buf.push((current_epoch, header_hash, header));
            }

            // Check validator node MR changes BEFORE epoch changes so that new registrations
            // are processed before assign_validators_for_epoch runs. This matters when the base
            // layer updates the validator_node_mr at epoch boundary blocks.
            if self.last_scanned_validator_node_mr != Some(current_validator_node_mr) {
                debug!(
                    target: LOG_TARGET,
                    "⛓️ last_scanned_validator_node_mr = {} current = {}", self.last_scanned_validator_node_mr.display(), current_validator_node_mr
                );

                if self.config.features.sync_validator_node_changes {
                    let node_changes = self
                        .base_node_client
                        .get_validator_node_changes(current_epoch, self.config.sidechain_id.as_ref())
                        .await
                        .map_err(BaseLayerOracleError::BaseNodeError)?;
                    let node_changes = node_changes
                        .into_iter()
                        .map(TryInto::try_into)
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| {
                            BaseLayerOracleError::InvalidBaseNodeResponse(format!(
                                "Failed to convert validator node change: {}",
                                e
                            ))
                        })?;

                    // This maybe empty if the MR changed as a result of other side chain IDs
                    if !node_changes.is_empty() {
                        self.pending_events
                            .push_back(EpochEvent::ActiveValidatorNodeSetChanged {
                                epoch: current_epoch,
                                node_changes,
                            });
                    }
                }
                self.last_scanned_validator_node_mr = Some(current_validator_node_mr);
            }

            if header_height % constants.epoch_length() == 0 {
                info!(
                    target: LOG_TARGET,
                    "🟩 New epoch block {} {} {}", current_epoch, header_height, header_hash
                );
                self.last_epoch_hash = Some(header_hash);

                // TODO: we do not handle consensus constants changing during scan here for performance reasons.
                // constants = self.base_node_client.get_consensus_constants(header_height).await?;

                // Emit EpochChanged for every boundary so every (epoch, epoch_hash) pair is persisted
                // by the epoch manager. Consensus's get_epoch_hash(epoch) lookup depends on that
                // row being present for any epoch it may transition into — including epochs
                // crossed during a long catch-up.
                info!(
                    target: LOG_TARGET,
                    "🟩 epoch change {}->{} (height({}) hash({}))", scan_epoch, current_epoch, header_height, header_hash
                );
                self.pending_events.push_back(EpochEvent::EpochChanged {
                    epoch: current_epoch,
                    // Set above
                    epoch_hash: self.last_epoch_hash.unwrap_or_default(),
                });
                scan_epoch = current_epoch;
            }

            // Track incremental progress of hash/height so a mid-loop failure can resume from the
            // last processed header. `last_scanned_tip` is intentionally NOT updated here — it
            // represents the un-lagged tip we've fully caught up to, and is set only by
            // set_last_scanned_block below once the whole batch has been persisted. Otherwise an
            // interrupted sync would cause get_blockchain_progression to short-circuit on the next
            // round and silently skip the remaining headers.
            self.last_scanned_hash = Some(header_hash);
            self.last_scanned_height = header_height;
        }

        if !self.header_buf.is_empty() {
            self.store
                .add_block_headers(self.header_buf.drain(..))
                .map_err(BaseLayerOracleError::StoreError)?;
        }
        self.set_last_scanned_block(
            self.last_scanned_hash.unwrap_or_else(FixedHash::zero),
            self.last_scanned_height,
            tip.tip_hash,
        )?;

        if let Some(hash) = self.last_epoch_hash {
            self.set_last_epoch_block(hash)?;
        }

        // let scan_height = constants.epoch_to_height(scan_epoch);
        // if self.last_scanned_validator_node_mr.is_none() {
        //     // Initial validator node download
        //     let mut stream = self.base_node_client.get_validator_nodes(scan_height).await?;
        //     while let Some(node) = stream.try_next().await? {
        //         self.pending_events
        //             .push_back(EpochEvent::ActiveValidatorNodeSetChanged {
        //                 epoch: scan_epoch,
        //                 node_changes: vec![ValidatorNodeChange::Add {
        //                     claim_public_key: node.public_key,
        //                     validator_node_public_key: node.public_key,
        //                     activation_epoch: scan_epoch,
        //                     minimum_value_promise: 0,
        //                     shard_key: node.shard_key,
        //                 }],
        //             });
        //     }
        // }

        if self.last_scanned_height < lag_tip_height {
            info!(
                target: LOG_TARGET,
                "⛓️ Completed scanning up to height {}. Continuing scan to catch up to lagged tip height {}.",
                self.last_scanned_height,
                lag_tip_height
            );
            return Ok(true);
        }

        let latest = self.base_node_client.get_tip_info().await?;
        let lagged_height = latest.height_of_longest_chain.saturating_sub(self.config.height_lag);
        if lagged_height > lag_tip_height {
            info!(
                target: LOG_TARGET,
                "Base layer blockchain has progressed since the last scan. Last scanned block height: {}. \
                 Latest block height: {}",
                lag_tip_height,
                latest.height_of_longest_chain
            );
            Ok(true)
        } else {
            self.pending_events.push_back(EpochEvent::DoneForNow {
                epoch: scan_epoch,
                epoch_hash: self.last_epoch_hash.unwrap_or_else(FixedHash::zero),
            });
            Ok(false)
        }
    }

    fn set_last_scanned_block(
        &mut self,
        header_hash: FixedHash,
        height: u64,
        tip: FixedHash,
    ) -> Result<(), BaseLayerOracleError> {
        self.store
            .set(StoreKey::BaseLayerLastScannedTip.as_key_bytes(), &tip)
            .map_err(BaseLayerOracleError::StoreError)?;
        self.store
            .set(StoreKey::BaseLayerLastScannedBlockHash.as_key_bytes(), &header_hash)
            .map_err(BaseLayerOracleError::StoreError)?;
        self.store
            .set(StoreKey::BaseLayerLastScannedBlockHeight.as_key_bytes(), &height)
            .map_err(BaseLayerOracleError::StoreError)?;
        self.last_scanned_tip = Some(tip);
        self.last_scanned_hash = Some(header_hash);
        self.last_scanned_height = height;
        Ok(())
    }

    fn set_last_epoch_block(&mut self, hash: FixedHash) -> Result<(), BaseLayerOracleError> {
        self.store
            .set(StoreKey::BaseLayerLastEpochHash.as_key_bytes(), &hash)
            .map_err(BaseLayerOracleError::StoreError)?;
        self.last_epoch_hash = Some(hash);
        Ok(())
    }

    fn shrink_events(&mut self) {
        const CAP_TO_SHRINK: usize = 1000;
        const TARGET_CAP: usize = 500;
        if self.pending_events.capacity() >= CAP_TO_SHRINK {
            self.pending_events.shrink_to(TARGET_CAP);
        }
    }

    /// Returns true when our lagged scanner position is within `epoch_end_spread_blocks` of the
    /// next epoch boundary. Used by consensus to accept `EndEpoch` proposals speculatively when
    /// peers' oracles have already crossed and ours is almost there.
    fn is_within_epoch_end_spread(&self, current_epoch: Epoch) -> bool {
        if self.config.epoch_end_spread_blocks == 0 {
            return false;
        }
        let Some(epoch_length) = self.cached_epoch_length else {
            return false;
        };
        if epoch_length == 0 {
            return false;
        }
        let Some(next_start) = current_epoch
            .as_u64()
            .checked_add(1)
            .and_then(|e| e.checked_mul(epoch_length))
        else {
            return false;
        };
        self.last_scanned_height
            .saturating_add(self.config.epoch_end_spread_blocks) >=
            next_start
    }
}

impl<TStore: EpochOracleStore + BaseLayerBlockHeaderStore + Send + 'static, TClient: BaseNodeClient + 'static>
    BaseLayerOracle<TStore, TClient>
{
    fn poll_next_event(&mut self, cx: &mut Context) -> Poll<Option<EpochEvent>> {
        if self.is_done {
            return Poll::Ready(None);
        }

        if !self.is_initialized {
            match self
                .inner
                .as_mut()
                .expect("inner must be Some if not initialized")
                .load_initial_state()
            {
                Ok(()) => {
                    trace!(target: LOG_TARGET, "Initialized");
                    self.is_initialized = true;
                },
                Err(err) => {
                    self.is_done = true;
                    return Poll::Ready(Some(EpochEvent::error(err)));
                },
            }
        }

        loop {
            if let Some(inner_mut) = self.inner.as_mut() {
                if let Some(event) = inner_mut.pending_events.pop_front() {
                    trace!(target: LOG_TARGET, "Pop event {event}");
                    return Poll::Ready(Some(event));
                }
                inner_mut.shrink_events();
            }

            if let Some(mut task) = self.task.take() {
                trace!(target: LOG_TARGET, "Work on sync");
                match task.as_mut().poll(cx) {
                    Poll::Ready((Ok(has_more), inner)) => {
                        trace!(target: LOG_TARGET, "Sync complete Ok");
                        self.inner = Some(inner);
                        self.has_more = has_more;
                        trace!(target: LOG_TARGET, "has_more={has_more}");
                        // There may be events to return, do that straight away
                        continue;
                    },
                    Poll::Ready((Err(err), inner)) => {
                        debug!(target: LOG_TARGET, "Sync complete Err({err})");
                        self.inner = Some(inner);
                        return Poll::Ready(Some(EpochEvent::error(err)));
                    },
                    Poll::Pending => {
                        self.task = Some(task);
                        return Poll::Pending;
                    },
                }
            }

            // On the first call of this method, sleep_or_shutdown is false and scanning will immediately begin
            if !self.has_more && self.sleep_or_shutdown && self.sleep_task.is_none() {
                let scanning_interval = self
                    .inner
                    .as_ref()
                    .expect("inner must be Some when starting sleep task")
                    .config
                    .scanning_interval;
                trace!(target: LOG_TARGET, "Starting sleep task {:?}", scanning_interval);
                self.sleep_task = Some(Box::pin(time::sleep(scanning_interval)));
            }

            if let Some(sleep) = self.sleep_task.as_mut() {
                trace!(target: LOG_TARGET, "Sleep poll");
                if sleep.as_mut().poll(cx).is_pending() {
                    return Poll::Pending;
                }
            }
            self.sleep_task = None;

            let mut inner = self.inner.take().expect("inner is None");
            let has_more = self.has_more;
            self.task = Some(Box::pin(async move {
                trace!(target: LOG_TARGET, "Starting blockchain scan task");
                let result = inner.scan_blockchain(has_more).await;
                (result, inner)
            }));

            self.sleep_or_shutdown = true;
        }
    }
}

impl<TStore: EpochOracleStore + BaseLayerBlockHeaderStore + Send + 'static, TClient: BaseNodeClient + 'static>
    EpochEventOracle for BaseLayerOracle<TStore, TClient>
{
    /// Returns a Future that returns the next event, completing a round of scanning if necessary.
    /// This Future is cancel-safe. Returns None if a shutdown is triggered.
    async fn next_epoch_event(&mut self) -> Option<EpochEvent> {
        poll_fn(|cx| self.poll_next_event(cx)).await
    }

    fn is_within_epoch_end_spread(&self, current_epoch: Epoch) -> bool {
        // `inner` is briefly None while a scan task is in flight; return false then so voting
        // falls back to the strict em_epoch > current_epoch check.
        self.inner
            .as_deref()
            .map(|inner| inner.is_within_epoch_end_spread(current_epoch))
            .unwrap_or(false)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BaseLayerOracleError {
    #[error("Store error: {0}")]
    StoreError(anyhow::Error),
    #[error("Base node client error: {0}")]
    BaseNodeError(#[from] BaseNodeClientError),
    #[error("Invalid base node response: {0}")]
    InvalidBaseNodeResponse(String),
}

enum BlockchainProgression {
    /// The blockchain has progressed since the last scan
    Progressed,
    /// Reorg was detected
    Reorged,
    /// The blockchain has not progressed since the last scan
    NoProgress,
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, VecDeque},
        sync::{Arc, Mutex},
        time::Duration,
    };

    use ootle_network::Network;
    use tari_base_node_client::{
        BaseNodeClient,
        BaseNodeClientError,
        ValidatorNodeChange,
        futures_util::stream::{self, Stream},
        tonic,
        types::{BaseLayerConsensusConstants, BaseLayerMetadata, BaseLayerValidatorNode, SideChainUtxos},
    };
    use tari_common_types::types::FixedHash;
    use tari_node_components::blocks::BlockHeader;
    use tari_ootle_common_types::Epoch;
    use tari_ootle_storage::global::BlockHeaderModel;
    use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;
    use tari_transaction_components::{tari_amount::MicroMinotari, transaction_components::CodeTemplateRegistration};

    use super::{BaseLayerOracleInner, hash_header};
    use crate::{
        base_layer::{
            BaseLayerBlockHeaderStore,
            config::{BaseLayerEpochOracleConfig, BaseLayerEpochOracleFeatures},
        },
        store::EpochOracleStore,
    };

    const NETWORK: Network = Network::LocalNet;
    const EPOCH_LENGTH: u64 = 5;

    /// In-memory implementation of the oracle's store traits. Cloneable: the test holds one handle for
    /// assertions while the oracle owns another.
    #[derive(Clone, Default)]
    struct InMemoryStore {
        metadata: Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>,
        headers: Arc<Mutex<Vec<BlockHeaderModel>>>,
    }

    impl InMemoryStore {
        /// Sorted list of stored header heights.
        fn heights(&self) -> Vec<u64> {
            let mut heights: Vec<u64> = self.headers.lock().unwrap().iter().map(|m| m.height).collect();
            heights.sort_unstable();
            heights
        }

        fn hash_at(&self, height: u64) -> Option<FixedHash> {
            self.headers
                .lock()
                .unwrap()
                .iter()
                .find(|m| m.height == height)
                .map(|m| m.block_hash)
        }
    }

    impl EpochOracleStore for InMemoryStore {
        fn get<T: serde::de::DeserializeOwned>(&self, key: &[u8]) -> anyhow::Result<Option<T>> {
            match self.metadata.lock().unwrap().get(key) {
                Some(bytes) => Ok(Some(serde_json::from_slice(bytes)?)),
                None => Ok(None),
            }
        }

        fn set<T: serde::Serialize>(&self, key: &[u8], value: &T) -> anyhow::Result<()> {
            self.metadata
                .lock()
                .unwrap()
                .insert(key.to_vec(), serde_json::to_vec(value)?);
            Ok(())
        }
    }

    impl BaseLayerBlockHeaderStore for InMemoryStore {
        fn add_block_headers<I: IntoIterator<Item = (Epoch, FixedHash, BlockHeader)>>(
            &self,
            headers: I,
        ) -> anyhow::Result<()> {
            let mut stored = self.headers.lock().unwrap();
            for (epoch, block_hash, header) in headers {
                // Mirror the SQL on_conflict(block_hash, epoch).do_nothing() idempotency.
                if stored.iter().any(|m| m.block_hash == block_hash && m.epoch == epoch) {
                    continue;
                }
                stored.push(BlockHeaderModel {
                    epoch,
                    height: header.height,
                    block_hash,
                    kernel_merkle_root: header.kernel_mr,
                    validator_node_merkle_root: header.validator_node_mr,
                });
            }
            Ok(())
        }

        fn find_block_header_by_hash(
            &self,
            max_epoch: Epoch,
            block_hash: &FixedHash,
        ) -> anyhow::Result<Option<BlockHeaderModel>> {
            Ok(self
                .headers
                .lock()
                .unwrap()
                .iter()
                .find(|m| m.block_hash == *block_hash && m.epoch <= max_epoch)
                .cloned())
        }

        fn get_first_block_header_in_epoch(&self, epoch: Epoch) -> anyhow::Result<Option<BlockHeaderModel>> {
            Ok(self
                .headers
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.epoch == epoch)
                .min_by_key(|m| m.height)
                .cloned())
        }

        fn delete_block_headers_above(&self, height: u64) -> anyhow::Result<usize> {
            let mut stored = self.headers.lock().unwrap();
            let before = stored.len();
            stored.retain(|m| m.height <= height);
            Ok(before - stored.len())
        }
    }

    /// Mock base node serving a configurable canonical chain (indexed by height). Swapping the chain
    /// simulates a reorg.
    #[derive(Clone)]
    struct MockBaseNode {
        chain: Arc<Mutex<Vec<BlockHeader>>>,
    }

    impl MockBaseNode {
        fn new(chain: Vec<BlockHeader>) -> Self {
            Self {
                chain: Arc::new(Mutex::new(chain)),
            }
        }

        fn set_chain(&self, chain: Vec<BlockHeader>) {
            *self.chain.lock().unwrap() = chain;
        }
    }

    impl BaseNodeClient for MockBaseNode {
        async fn test_connection(&mut self) -> Result<(), BaseNodeClientError> {
            Ok(())
        }

        async fn get_network(&mut self) -> Result<u8, BaseNodeClientError> {
            Ok(NETWORK.as_byte())
        }

        async fn get_tip_info(&mut self) -> Result<BaseLayerMetadata, BaseNodeClientError> {
            let chain = self.chain.lock().unwrap();
            let tip = chain.last().expect("chain must not be empty");
            Ok(BaseLayerMetadata {
                height_of_longest_chain: tip.height,
                tip_hash: hash_header(NETWORK, tip),
            })
        }

        async fn get_validator_node_changes(
            &mut self,
            _epoch: Epoch,
            _sidechain_id: Option<&RistrettoPublicKeyBytes>,
        ) -> Result<Vec<ValidatorNodeChange>, BaseNodeClientError> {
            Ok(Vec::new())
        }

        async fn get_validator_nodes(
            &mut self,
            _height: u64,
        ) -> Result<impl Stream<Item = Result<BaseLayerValidatorNode, BaseNodeClientError>> + Send, BaseNodeClientError>
        {
            Ok(stream::iter(
                Vec::<Result<BaseLayerValidatorNode, BaseNodeClientError>>::new(),
            ))
        }

        async fn get_template_registrations(
            &mut self,
            _start_hash: Option<FixedHash>,
            _count: u64,
        ) -> Result<Vec<CodeTemplateRegistration>, BaseNodeClientError> {
            unimplemented!("not used by the reorg tests")
        }

        async fn get_header_by_hash(&mut self, block_hash: &FixedHash) -> Result<BlockHeader, BaseNodeClientError> {
            self.chain
                .lock()
                .unwrap()
                .iter()
                .find(|h| hash_header(NETWORK, h) == *block_hash)
                .cloned()
                .ok_or(BaseNodeClientError::GrpcStatus {
                    code: tonic::Code::NotFound,
                    message: "header not found".to_string(),
                })
        }

        async fn stream_headers(
            &mut self,
            from_height: u64,
            limit: u64,
        ) -> Result<impl Stream<Item = Result<BlockHeader, BaseNodeClientError>> + Unpin + Send, BaseNodeClientError>
        {
            let chain = self.chain.lock().unwrap();
            let headers: Vec<_> = chain
                .iter()
                .filter(|h| h.height >= from_height && h.height < from_height.saturating_add(limit))
                .cloned()
                .map(Ok)
                .collect();
            Ok(stream::iter(headers))
        }

        async fn get_consensus_constants(
            &mut self,
            _tip: u64,
        ) -> Result<BaseLayerConsensusConstants, BaseNodeClientError> {
            Ok(BaseLayerConsensusConstants {
                epoch_length: EPOCH_LENGTH,
                validator_node_registration_min_deposit_amount: MicroMinotari::from(0u64),
            })
        }

        async fn get_sidechain_utxos(
            &mut self,
            _start_hash: Option<FixedHash>,
            _count: u64,
        ) -> Result<Vec<SideChainUtxos>, BaseNodeClientError> {
            unimplemented!("not used by the reorg tests")
        }
    }

    fn make_header(height: u64, salt: u64) -> BlockHeader {
        let mut header = BlockHeader::new(0);
        header.height = height;
        // nonce feeds hash_header, so distinct salts give distinct block hashes.
        header.nonce = salt;
        header
    }

    fn make_inner(
        store: InMemoryStore,
        client: MockBaseNode,
        height_lag: u64,
    ) -> BaseLayerOracleInner<InMemoryStore, MockBaseNode> {
        BaseLayerOracleInner {
            config: BaseLayerEpochOracleConfig {
                start_height: 0,
                height_lag,
                scanning_interval: Duration::from_secs(1),
                sidechain_id: None,
                features: BaseLayerEpochOracleFeatures {
                    sync_headers: true,
                    sync_validator_node_changes: false,
                },
                epoch_end_spread_blocks: 0,
            },
            store,
            last_scanned_height: 0,
            last_scanned_tip: None,
            last_scanned_hash: None,
            last_epoch_hash: None,
            last_scanned_validator_node_mr: None,
            base_node_client: client,
            has_attempted_scan: false,
            pending_events: VecDeque::new(),
            header_buf: Vec::new(),
            network: NETWORK,
            cached_epoch_length: None,
        }
    }

    /// Drives the scanner until it has fully caught up with the tip, mirroring the poll loop's
    /// `scan_blockchain(has_more)` driving.
    async fn sync_to_tip(inner: &mut BaseLayerOracleInner<InMemoryStore, MockBaseNode>) {
        let mut has_more = false;
        loop {
            has_more = inner.scan_blockchain(has_more).await.unwrap();
            inner.pending_events.clear();
            if !has_more {
                break;
            }
        }
    }

    fn linear_chain(heights: std::ops::RangeInclusive<u64>) -> Vec<BlockHeader> {
        heights.map(|h| make_header(h, h)).collect()
    }

    #[tokio::test]
    async fn reorg_rewinds_to_fork_point_and_prunes_orphans() {
        // height_lag = 5, tip = 20 -> lagged tip = 15. Scans heights 1..=15.
        let store = InMemoryStore::default();
        let chain_a = linear_chain(0..=20);
        let client = MockBaseNode::new(chain_a.clone());
        let mut inner = make_inner(store.clone(), client.clone(), 5);

        sync_to_tip(&mut inner).await;
        assert_eq!(inner.last_scanned_height, 15);
        assert_eq!(store.heights(), (1..=15).collect::<Vec<_>>());

        // Reorg diverging at height 13 (fork point 12), within the recoverable depth (15 - 5 = 10).
        let mut chain_b = chain_a[..=12].to_vec();
        chain_b.extend((13..=20).map(|h| make_header(h, h + 10_000)));
        client.set_chain(chain_b.clone());

        let tip_b = inner.base_node_client.get_tip_info().await.unwrap();
        inner.handle_reorg(&tip_b).await.unwrap();

        // Orphaned headers 13..=15 pruned; scan rewound to the fork point.
        assert_eq!(inner.last_scanned_height, 12);
        assert_eq!(store.heights(), (1..=12).collect::<Vec<_>>());
        assert_eq!(inner.last_scanned_hash, Some(hash_header(NETWORK, &chain_b[12])));
        // last_epoch_hash reset to the boundary that opened the fork epoch (epoch 2 -> height 10),
        // not left at the orphaned epoch-3 boundary (height 15).
        assert_eq!(inner.last_epoch_hash, Some(hash_header(NETWORK, &chain_a[10])));
    }

    #[tokio::test]
    async fn reorg_scan_converges_on_new_chain() {
        let store = InMemoryStore::default();
        let chain_a = linear_chain(0..=20);
        let client = MockBaseNode::new(chain_a.clone());
        let mut inner = make_inner(store.clone(), client.clone(), 5);

        sync_to_tip(&mut inner).await;

        let mut chain_b = chain_a[..=12].to_vec();
        chain_b.extend((13..=20).map(|h| make_header(h, h + 10_000)));
        client.set_chain(chain_b.clone());

        // A full scan round detects the reorg, repairs the store, and re-scans the new chain.
        sync_to_tip(&mut inner).await;

        assert_eq!(inner.last_scanned_height, 15);
        assert_eq!(store.heights(), (1..=15).collect::<Vec<_>>());
        // Heights above the fork now carry chain B's hashes; the orphaned chain A headers are gone.
        for h in 13..=15u64 {
            assert_eq!(store.hash_at(h), Some(hash_header(NETWORK, &chain_b[h as usize])));
        }
        // Heights at and below the fork are untouched.
        for h in 1..=12u64 {
            assert_eq!(store.hash_at(h), Some(hash_header(NETWORK, &chain_a[h as usize])));
        }
    }

    #[tokio::test]
    async fn reorg_deeper_than_lag_triggers_full_rescan() {
        let store = InMemoryStore::default();
        let chain_a = linear_chain(0..=20);
        let client = MockBaseNode::new(chain_a.clone());
        let mut inner = make_inner(store.clone(), client.clone(), 5);

        sync_to_tip(&mut inner).await;
        assert_eq!(inner.last_scanned_height, 15);

        // Diverge at height 8 (fork point 7), below the recoverable range (15 - 5 = 10): the scanner
        // cannot find a common ancestor in [11, 15] and rebuilds from the start height.
        let mut chain_b = chain_a[..=7].to_vec();
        chain_b.extend((8..=20).map(|h| make_header(h, h + 10_000)));
        client.set_chain(chain_b);

        let tip_b = inner.base_node_client.get_tip_info().await.unwrap();
        inner.handle_reorg(&tip_b).await.unwrap();

        assert!(store.heights().is_empty());
        assert_eq!(inner.last_scanned_height, 0);
        assert_eq!(inner.last_scanned_hash, None);
    }
}
