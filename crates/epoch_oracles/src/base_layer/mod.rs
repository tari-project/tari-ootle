//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

mod header_hasher;

use std::{
    collections::VecDeque,
    future::{poll_fn, Future},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use log::*;
use tari_base_node_client::{
    grpc::GrpcBaseNodeClient,
    types::{BaseLayerMetadata, BlockInfo},
    BaseNodeClient,
    BaseNodeClientError,
};
use tari_common_types::types::FixedHash;
use tari_epoch_manager::epoch_event_oracle::{BlockHeaderData, EpochEvent, EpochEventOracle};
use tari_ootle_common_types::{displayable::Displayable, optional::Optional, Network};
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;
use tari_transaction_components::transaction_components::{
    OutputType,
    SideChainFeature,
    SideChainFeatureData,
    TransactionOutput,
};
use tari_utilities::ByteArray;
use tokio::time;

use crate::{
    base_layer::header_hasher::hash_header,
    store::{EpochOracleStore, StoreKey},
};

const LOG_TARGET: &str = "tari::ootle::base_layer_scanner";

type TaskOutput<TStore> = (Result<(), BaseLayerOracleError>, Box<BaseLayerOracleInner<TStore>>);

pub struct BaseLayerOracle<TStore> {
    inner: Option<Box<BaseLayerOracleInner<TStore>>>,
    scanning_interval: Duration,
    is_initialized: bool,
    is_done: bool,
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
    next_block_hash: Option<FixedHash>,
    base_node_client: GrpcBaseNodeClient,
    height_lag: u64,
    has_attempted_scan: bool,
    validator_node_sidechain_id: Option<RistrettoPublicKeyBytes>,
    burnt_utxo_sidechain_id: Option<RistrettoPublicKeyBytes>,
    template_sidechain_id: Option<RistrettoPublicKeyBytes>,
    pending_events: VecDeque<EpochEvent>,
    network: Network,
}

impl<TStore: EpochOracleStore + 'static> BaseLayerOracle<TStore> {
    pub fn new(
        store: TStore,
        base_node_client: GrpcBaseNodeClient,
        height_lag: u64,
        scanning_interval: Duration,
        validator_node_sidechain_id: Option<RistrettoPublicKeyBytes>,
        burnt_utxo_sidechain_id: Option<RistrettoPublicKeyBytes>,
        template_sidechain_id: Option<RistrettoPublicKeyBytes>,
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
                next_block_hash: None,
                base_node_client,
                height_lag,
                has_attempted_scan: false,
                validator_node_sidechain_id,
                burnt_utxo_sidechain_id,
                template_sidechain_id,
                pending_events: VecDeque::new(),
                network,
            })),
            scanning_interval,
            is_initialized: false,
            is_done: false,
            sleep_or_shutdown: false,
            task: None,
            sleep_task: None,
        }
    }
}

impl<TStore: EpochOracleStore> BaseLayerOracleInner<TStore> {
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
            .get(StoreKey::BaseLayerLastScannedBlockHash.as_key_bytes())
            .map_err(BaseLayerOracleError::StoreError)?;
        self.last_scanned_height = self
            .store
            .get(StoreKey::BaseLayerLastScannedBlockHeight.as_key_bytes())
            .map_err(BaseLayerOracleError::StoreError)?
            .unwrap_or(0);
        self.next_block_hash = self
            .store
            .get(StoreKey::BaseLayerNextBlockHash.as_key_bytes())
            .map_err(BaseLayerOracleError::StoreError)?;
        Ok(())
    }

    async fn scan_blockchain(&mut self) -> Result<(), BaseLayerOracleError> {
        // fetch the new base layer info since the previous scan
        let tip = self.base_node_client.get_tip_info().await?;

        match self.get_blockchain_progression(&tip).await? {
            BlockchainProgression::Progressed => {
                info!(
                    target: LOG_TARGET,
                    "⛓️ Blockchain has progressed to height {}. We last scanned {}/{}. Scanning for new side-chain \
                     UTXOs.",
                    tip.height_of_longest_chain,
                    self.last_scanned_height,
                    tip.height_of_longest_chain
                        .saturating_sub(self.height_lag)
                );
                self.sync_blockchain(tip).await?;
            },
            BlockchainProgression::Reorged => {
                error!(
                    target: LOG_TARGET,
                    "⚠️ Base layer reorg detected. Rescanning from genesis."
                );
                // TODO: we need to figure out where the fork happened, and delete data after the fork if able.
                self.last_scanned_hash = None;
                self.last_scanned_validator_node_mr = None;
                self.last_scanned_height = 0;
                self.sync_blockchain(tip).await?;
            },
            BlockchainProgression::NoProgress => {
                trace!(target: LOG_TARGET, "No new blocks to scan.");
                if !self.has_attempted_scan {
                    let constants = self
                        .base_node_client
                        .get_consensus_constants(tip.height_of_longest_chain)
                        .await?;
                    let lagged_height = tip.height_of_longest_chain.saturating_sub(self.height_lag);
                    let epoch = constants.height_to_epoch(lagged_height);
                    // If no progress has been made since restarting, we still need to tell the epoch manager that
                    // scanning is done
                    self.pending_events.push_back(EpochEvent::DoneForNow {
                        epoch,
                        epoch_hash: self.last_epoch_hash.unwrap_or_else(FixedHash::zero),
                    });
                }
            },
        }

        self.has_attempted_scan = true;

        Ok(())
    }

    async fn get_blockchain_progression(
        &mut self,
        tip: &BaseLayerMetadata,
    ) -> Result<BlockchainProgression, BaseLayerOracleError> {
        if tip.height_of_longest_chain == 0 {
            return Ok(BlockchainProgression::NoProgress);
        }
        match &self.last_scanned_tip {
            Some(hash) if *hash == tip.tip_hash => Ok(BlockchainProgression::NoProgress),
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
    async fn sync_blockchain(&mut self, tip: BaseLayerMetadata) -> Result<(), BaseLayerOracleError> {
        let start_scan_height = self.last_scanned_height;
        let mut current_hash = self.last_scanned_hash;
        let end_height = match tip.height_of_longest_chain.checked_sub(self.height_lag) {
            None => {
                debug!(
                    target: LOG_TARGET,
                    "Base layer blockchain is not yet at the required height to start scanning it"
                );
                return Ok(());
            },
            Some(end_height) => end_height,
        };

        let mut cached_header = None;
        // Recover the last scanned validator node MR if it is not set yet, i.e the node has scanned BL blocks
        // previously.
        if self.last_scanned_validator_node_mr.is_none() {
            if let Some(ref hash) = self.last_scanned_hash {
                let header = self.base_node_client.get_header_by_hash(hash).await?;
                self.last_scanned_validator_node_mr = Some(header.validator_node_mr);
                cached_header = Some(header);
            }
        }

        let mut constants = self
            .base_node_client
            .get_consensus_constants(tip.height_of_longest_chain)
            .await?;
        let mut initial_epoch = constants.height_to_epoch(start_scan_height);
        for current_height in start_scan_height..=end_height {
            let utxos = self
                .base_node_client
                .get_sidechain_utxos(current_hash, 1)
                .await?
                .pop()
                .ok_or_else(|| {
                    BaseLayerOracleError::InvalidBaseNodeResponse(format!(
                        "Base layer returned empty response for height {}",
                        current_height
                    ))
                })?;
            let block_info = utxos.block_info;

            // TODO: Because we don't know the next hash when we're done scanning to the tip, we need to load the
            //       previous scanned block again to get it. Won't be an issue when we scan a few
            //       blocks back.
            if self.last_scanned_hash.is_some_and(|h| h == block_info.hash) {
                if let Some(hash) = block_info.next_block_hash {
                    current_hash = Some(hash);
                    continue;
                }
                break;
            }
            info!(
                target: LOG_TARGET,
                "⛓️ Scanning base layer block {} of {}", block_info.height, end_height
            );

            let header = match cached_header {
                Some(ref h) if h.prev_hash == block_info.hash => h,
                _ => {
                    let header = self.base_node_client.get_header_by_hash(&block_info.hash).await?;
                    let current_epoch = constants.height_to_epoch(current_height);
                    self.pending_events.push_back(EpochEvent::NewBlockHeader {
                        epoch: current_epoch,
                        header: BlockHeaderData {
                            height: header.height,
                            // TODO: Cant use header.hash() because it uses CURRENT_NETWORK global
                            hash: hash_header(self.network, &header),
                            kernel_merkle_root: header.kernel_mr,
                            validator_node_merkle_root: header.validator_node_mr,
                        },
                    });
                    cached_header = Some(header);
                    cached_header.as_ref().unwrap()
                },
            };
            let current_validator_node_mr = header.validator_node_mr;
            let current_epoch = constants.height_to_epoch(current_height);

            if block_info.height % constants.epoch_length == 0 {
                info!(
                    target: LOG_TARGET,
                    "🟩 New epoch block {} {} {}", current_epoch, block_info.height, block_info.hash
                );
                self.set_last_epoch_block(block_info.hash)?;
            }

            for output in utxos.outputs {
                let output_hash = output.hash();
                let TransactionOutput {
                    features, commitment, ..
                } = output;
                let Some(SideChainFeature {
                    data: sidechain_feature_data,
                    sidechain_id,
                }) = features.sidechain_feature
                else {
                    warn!(target: LOG_TARGET, "Base node returned invalid data: Sidechain utxo output must have sidechain features");
                    continue;
                };

                match sidechain_feature_data {
                    SideChainFeatureData::ValidatorNodeRegistration(reg) => {
                        if sidechain_id.as_ref().map(|s| s.public_key().as_bytes()) !=
                            self.validator_node_sidechain_id.as_ref().map(|p| p.as_bytes())
                        {
                            debug!(
                                target: LOG_TARGET,
                                "Ignoring VN reg for sidechain ID {}.",
                                sidechain_id.as_ref().map(|id| id.public_key()).display()
                            );
                            continue;
                        }

                        debug!(target: LOG_TARGET, "🖥️ New validator node registration at height {}: {}", current_height, reg.public_key());
                        self.pending_events.push_back(EpochEvent::NewValidatorRegistered {
                            epoch: current_epoch,
                            claim_public_key: RistrettoPublicKeyBytes::from_bytes(reg.claim_public_key().as_bytes())
                                .expect(
                                    "claim_public_key: Compressed<RistrettoPublicKey> and RistrettoPublicKeyBytes \
                                     must be the same length",
                                ),
                            validator_node_public_key: RistrettoPublicKeyBytes::from_bytes(reg.public_key().as_bytes())
                                .expect(
                                    "validator_node_public_key: Compressed<RistrettoPublicKey> and \
                                     RistrettoPublicKeyBytes must be the same length",
                                ),
                        });
                    },

                    SideChainFeatureData::CodeTemplateRegistration(registration) => {
                        if sidechain_id.as_ref().map(|s| s.public_key().as_bytes()) !=
                            self.template_sidechain_id.as_ref().map(|p| p.as_bytes())
                        {
                            debug!(
                                target: LOG_TARGET,
                                "Ignoring CodeTemplateRegistration for sidechain ID {}.",
                                sidechain_id.as_ref().map(|id| id.public_key()).display()
                            );
                            continue;
                        }
                        debug!(
                            target: LOG_TARGET,
                            "🌠 new template found with hash {} at height {}", registration.binary_sha, block_info.height
                        );

                        // Nothing to do. Template registrations on minotari are deprecated
                    },
                    SideChainFeatureData::ConfidentialOutput(_) => {
                        if sidechain_id.as_ref().map(|s| s.public_key().as_bytes()) !=
                            self.burnt_utxo_sidechain_id.as_ref().map(|p| p.as_bytes())
                        {
                            debug!(
                                target: LOG_TARGET,
                                "Ignoring ConfidentialOutput for sidechain ID {}.",
                                sidechain_id.as_ref().map(|id| id.public_key()).display()
                            );
                            continue;
                        }
                        // Should be checked by the base layer
                        if !matches!(features.output_type, OutputType::Burn) {
                            warn!(
                                target: LOG_TARGET,
                                "Ignoring confidential output that is not burned: {} with commitment {}",
                                output_hash,
                                commitment.to_compressed_key(),
                            );
                            continue;
                        }

                        debug!(
                            target: LOG_TARGET,
                            "⛓️ Found burned output: {} with commitment {}",
                            output_hash,
                            commitment.to_compressed_key()
                        );
                    },
                    SideChainFeatureData::EvictionProof(eviction_proof) => {
                        if sidechain_id.as_ref().map(|s| s.public_key().as_bytes()) !=
                            self.validator_node_sidechain_id.as_ref().map(|p| p.as_bytes())
                        {
                            debug!(
                                target: LOG_TARGET,
                                "Ignoring EvictionProof for sidechain ID {}.",
                                sidechain_id.as_ref().map(|id| id.public_key()).display()
                            );
                            continue;
                        }
                        trace!(target: LOG_TARGET, "Eviction proof scanned: {eviction_proof:?}");
                        self.pending_events.push_back(EpochEvent::NewEvictionProof {
                            epoch: current_epoch,
                            eviction_proof,
                        })
                    },
                    SideChainFeatureData::ValidatorNodeExit(exit) => {
                        if sidechain_id.as_ref().map(|s| s.public_key().as_bytes()) !=
                            self.validator_node_sidechain_id.as_ref().map(|p| p.as_bytes())
                        {
                            debug!(
                                target: LOG_TARGET,
                                "Ignoring VN exit for sidechain ID {}.",
                                sidechain_id.as_ref().map(|id| id.public_key()).display()
                            );
                            continue;
                        }

                        debug!(target: LOG_TARGET, "🖥️ validator node exit: {}", exit.public_key());
                        self.pending_events.push_back(EpochEvent::NewValidatorNodeExit {
                            epoch: current_epoch,
                            validator_node_public_key: RistrettoPublicKeyBytes::from_bytes(
                                exit.public_key().as_bytes(),
                            )
                            .expect(
                                "validator_node_public_key: Compressed<RistrettoPublicKey> and \
                                 RistrettoPublicKeyBytes must be the same length",
                            ),
                        });
                    },
                }
            }

            debug!(
                target: LOG_TARGET,
                "⛓️ last_scanned_validator_node_mr = {} current = {}", self.last_scanned_validator_node_mr.display(), current_validator_node_mr
            );
            // if the validator node MR has changed, we need to update the active validator node set
            if self.last_scanned_validator_node_mr != Some(current_validator_node_mr) {
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
                self.last_scanned_validator_node_mr = Some(current_validator_node_mr);
            }
            if current_epoch > initial_epoch {
                constants = self.base_node_client.get_consensus_constants(current_height).await?;

                info!(
                    target: LOG_TARGET,
                    "🟩 epoch change {}->{} {} {}", initial_epoch, current_epoch, block_info.height, block_info.hash
                );
                self.pending_events.push_back(EpochEvent::EpochChanged {
                    epoch: current_epoch,
                    epoch_hash: self.last_epoch_hash.unwrap_or_default(),
                });
                initial_epoch = current_epoch;
            }

            self.set_last_scanned_block(tip.tip_hash, &block_info)?;

            match block_info.next_block_hash {
                Some(next_hash) => {
                    current_hash = Some(next_hash);
                },
                None => {
                    info!(
                        target: LOG_TARGET,
                        "⛓️ No more blocks to scan. Last scanned block height: {}", block_info.height
                    );
                    if block_info.height != end_height {
                        return Err(BaseLayerOracleError::InvalidBaseNodeResponse(format!(
                            "Expected to scan to height {}, but got to height {}",
                            end_height, block_info.height
                        )));
                    }
                    break;
                },
            }
        }

        let latest = self.base_node_client.get_tip_info().await?;
        let lagged_height = latest.height_of_longest_chain.saturating_sub(self.height_lag);
        if lagged_height > end_height {
            info!(
                target: LOG_TARGET,
                "Base layer blockchain has progressed since the last scan. Last scanned block height: {}. \
                 Latest block height: {}",
                end_height,
                latest.height_of_longest_chain
            );
        } else {
            let constants = self.base_node_client.get_consensus_constants(lagged_height).await?;
            let epoch = constants.height_to_epoch(lagged_height);
            self.pending_events.push_back(EpochEvent::DoneForNow {
                epoch,
                epoch_hash: current_hash.unwrap_or_else(FixedHash::zero),
            });
        }

        Ok(())
    }

    fn set_last_scanned_block(&mut self, tip: FixedHash, block_info: &BlockInfo) -> Result<(), BaseLayerOracleError> {
        self.store
            .set(StoreKey::BaseLayerLastScannedTip.as_key_bytes(), &tip)
            .map_err(BaseLayerOracleError::StoreError)?;
        self.store
            .set(StoreKey::BaseLayerLastScannedBlockHash.as_key_bytes(), &block_info.hash)
            .map_err(BaseLayerOracleError::StoreError)?;
        self.store
            .set(
                StoreKey::BaseLayerNextBlockHash.as_key_bytes(),
                &block_info.next_block_hash,
            )
            .map_err(BaseLayerOracleError::StoreError)?;
        self.store
            .set(
                StoreKey::BaseLayerLastScannedBlockHeight.as_key_bytes(),
                &block_info.height,
            )
            .map_err(BaseLayerOracleError::StoreError)?;
        self.last_scanned_tip = Some(tip);
        self.last_scanned_hash = Some(block_info.hash);
        self.next_block_hash = block_info.next_block_hash;
        self.last_scanned_height = block_info.height;
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
}

impl<TStore: EpochOracleStore + Send + 'static> BaseLayerOracle<TStore> {
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
                    Poll::Ready((Ok(()), inner)) => {
                        trace!(target: LOG_TARGET, "Sync complete Ok");
                        self.inner = Some(inner);
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
            if self.sleep_or_shutdown && self.sleep_task.is_none() {
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
            self.task = Some(Box::pin(async move {
                let result = inner.scan_blockchain().await;
                (result, inner)
            }));

            self.sleep_or_shutdown = true;
        }
    }
}

impl<TStore: EpochOracleStore + Send + 'static> EpochEventOracle for BaseLayerOracle<TStore> {
    /// Returns a Future that returns the next event, completing a round of scanning if necessary.
    /// This Future is cancel-safe. Returns None if a shutdown is triggered.
    async fn next_epoch_event(&mut self) -> Option<EpochEvent> {
        poll_fn(|cx| self.poll_next_event(cx)).await
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
