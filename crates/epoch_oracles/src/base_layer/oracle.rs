//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::VecDeque,
    future::{Future, poll_fn},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use log::*;
use tari_base_node_client::{
    BaseNodeClient,
    BaseNodeClientError,
    futures_util::TryStreamExt,
    grpc::GrpcBaseNodeClient,
    types::BaseLayerMetadata,
};
use tari_common_types::types::FixedHash;
use tari_epoch_manager::epoch_event_oracle::{EpochEvent, EpochEventOracle};
use tari_node_components::blocks::BlockHeader;
use tari_ootle_common_types::{Epoch, Network, displayable::Displayable, optional::Optional};
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;
use tokio::time;

use crate::{
    base_layer::{
        BaseLayerBlockHeaderStore,
        BaseLayerEpochOracleConfig,
        BaseLayerEpochOracleFeatures,
        header_hasher::hash_header,
    },
    store::{EpochOracleStore, StoreKey},
};

const LOG_TARGET: &str = "tari::ootle::epoch_oracles::base_layer_scanner";

type TaskOutput<TStore> = (Result<bool, BaseLayerOracleError>, Box<BaseLayerOracleInner<TStore>>);

#[allow(clippy::struct_excessive_bools)]
pub struct BaseLayerOracle<TStore> {
    inner: Option<Box<BaseLayerOracleInner<TStore>>>,
    scanning_interval: Duration,
    is_initialized: bool,
    is_done: bool,
    has_more: bool,
    sleep_or_shutdown: bool,
    task: Option<Pin<Box<dyn Future<Output = TaskOutput<TStore>> + Send>>>,
    sleep_task: Option<Pin<Box<time::Sleep>>>,
}

struct BaseLayerOracleInner<TStore> {
    store: TStore,
    last_scanned_height: u64,
    last_scanned_tip: Option<FixedHash>,
    last_scanned_hash: Option<FixedHash>,
    last_epoch_hash: Option<FixedHash>,
    last_scanned_validator_node_mr: Option<FixedHash>,
    base_node_client: GrpcBaseNodeClient,
    start_height: u64,
    height_lag: u64,
    has_attempted_scan: bool,
    features: BaseLayerEpochOracleFeatures,
    validator_node_sidechain_id: Option<RistrettoPublicKeyBytes>,
    pending_events: VecDeque<EpochEvent>,
    header_buf: Vec<(Epoch, FixedHash, BlockHeader)>,
    network: Network,
    /// Epoch length in base-layer blocks, cached from consensus constants on the first scan
    /// that obtains them. `None` until the first scan completes.
    cached_epoch_length: Option<u64>,
    epoch_end_spread_blocks: u64,
}

impl<TStore: EpochOracleStore + BaseLayerBlockHeaderStore + 'static> BaseLayerOracle<TStore> {
    pub fn new(
        store: TStore,
        base_node_client: GrpcBaseNodeClient,
        config: BaseLayerEpochOracleConfig,
        network: Network,
    ) -> Self {
        Self {
            inner: Some(Box::new(BaseLayerOracleInner {
                store,
                last_scanned_tip: None,
                last_scanned_height: 0,
                last_scanned_hash: None,
                last_epoch_hash: None,
                last_scanned_validator_node_mr: None,
                base_node_client,
                start_height: config.start_height,
                height_lag: config.height_lag,
                has_attempted_scan: false,
                validator_node_sidechain_id: config.sidechain_id,
                features: config.features,
                pending_events: VecDeque::new(),
                header_buf: Vec::new(),
                network,
                cached_epoch_length: None,
                epoch_end_spread_blocks: config.epoch_end_spread_blocks,
            })),
            scanning_interval: config.scanning_interval,
            is_initialized: false,
            is_done: false,
            has_more: false,
            sleep_or_shutdown: false,
            task: None,
            sleep_task: None,
        }
    }
}

impl<TStore: EpochOracleStore + BaseLayerBlockHeaderStore> BaseLayerOracleInner<TStore> {
    /// The configured start height adjusted for the height lag. This is the minimum height
    /// that the scanner will scan from.
    fn lag_start_height(&self) -> u64 {
        self.start_height.saturating_sub(self.height_lag)
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
                    .saturating_sub(self.height_lag)
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
                        .saturating_sub(self.height_lag)
                );
                self.has_attempted_scan = true;
                self.sync_blockchain(tip).await
            },
            BlockchainProgression::Reorged => {
                error!(
                    target: LOG_TARGET,
                    "⚠️ Base layer reorg detected. Rescanning from genesis."
                );
                // TODO: we need to figure out where the fork happened, and delete data after the fork if able.
                self.last_scanned_hash = None;
                self.last_scanned_validator_node_mr = None;
                self.last_scanned_height = self.start_height;
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
                    let lagged_height = tip.height_of_longest_chain.saturating_sub(self.height_lag);
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

    #[allow(clippy::too_many_lines)]
    async fn sync_blockchain(&mut self, tip: BaseLayerMetadata) -> Result<bool, BaseLayerOracleError> {
        let Some(lag_tip_height) = tip.height_of_longest_chain.checked_sub(self.height_lag) else {
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
            if self.features.sync_headers {
                self.header_buf.push((current_epoch, header_hash, header));
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

            // if the validator node MR has changed, we need to update the active validator node set
            if self.last_scanned_validator_node_mr != Some(current_validator_node_mr) {
                debug!(
                    target: LOG_TARGET,
                    "⛓️ last_scanned_validator_node_mr = {} current = {}", self.last_scanned_validator_node_mr.display(), current_validator_node_mr
                );

                if self.features.sync_validator_node_changes {
                    let node_changes = self
                        .base_node_client
                        .get_validator_node_changes(current_epoch, self.validator_node_sidechain_id.as_ref())
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
        let lagged_height = latest.height_of_longest_chain.saturating_sub(self.height_lag);
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
        if self.epoch_end_spread_blocks == 0 {
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
        self.last_scanned_height.saturating_add(self.epoch_end_spread_blocks) >= next_start
    }
}

impl<TStore: EpochOracleStore + BaseLayerBlockHeaderStore + Send + 'static> BaseLayerOracle<TStore> {
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
                trace!(target: LOG_TARGET, "Starting sleep task {:?}", self.scanning_interval);
                self.sleep_task = Some(Box::pin(time::sleep(self.scanning_interval)));
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

impl<TStore: EpochOracleStore + BaseLayerBlockHeaderStore + Send + 'static> EpochEventOracle
    for BaseLayerOracle<TStore>
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
