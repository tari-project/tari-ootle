//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Consensus hook that writes lightweight template metadata to a dedicated RocksDB column family
//! whenever a template substate is committed to a block.
//!
//! The metadata (name, author, hash, epoch) is later served to indexers that set the
//! `TEMPLATE_METADATA` flag in their `sync_state` request, allowing catalogue discovery
//! without transmitting full WASM binaries.

use log::{error, info, warn};
use tari_consensus::{hotstuff::HotStuffError, messages::HotstuffMessage, traits::hooks::ConsensusHooks};
use tari_engine_types::published_template::TemplateMetadata;
use tari_ootle_common_types::{NodeHeight, services::template_provider::TemplateProvider};
use tari_ootle_storage::{StateStore, StorageError, consensus_models::ValidBlock};
use tari_ootle_transaction::TransactionId;
use tari_state_store_rocksdb::{RocksDbStateStore, writer::RocksDbStateStoreWriteTransaction};
use tari_template_lib::types::TemplateAddress;
use tokio::sync::mpsc;

use crate::state_store_template_provider::StateStoreTemplateProvider;

const LOG_TARGET: &str = "tari::validator::consensus::template_metadata_hooks";

/// Consensus hook that forwards newly committed template addresses to a background worker.
///
/// All methods are non-blocking: `on_local_block_committed` sends template addresses through
/// an unbounded channel to [`TemplateMetadataWorker`], which performs the expensive WASM
/// loading and RocksDB writes off the consensus thread.
#[derive(Clone)]
pub struct TemplateMetadataHooks {
    tx_template_addresses: mpsc::UnboundedSender<TemplateAddress>,
}

impl TemplateMetadataHooks {
    pub fn new(tx_template_addresses: mpsc::UnboundedSender<TemplateAddress>) -> Self {
        Self { tx_template_addresses }
    }
}

impl ConsensusHooks for TemplateMetadataHooks {
    fn on_local_block_committed(&mut self, block: &ValidBlock) {
        // Collect template addresses committed in this block.
        let template_addresses = block
            .block()
            .commands()
            .iter()
            .filter_map(|c| c.committing())
            .flat_map(|a| a.evidence.all_outputs_iter())
            .filter(|(_, id, _)| id.is_template())
            .filter_map(|(_, id, _)| id.as_template())
            .map(|published_addr| published_addr.as_template_address());

        for address in template_addresses {
            // Non-blocking: if the receiver has dropped (shutdown), silently discard.
            let _ = self.tx_template_addresses.send(address);
        }
    }

    fn on_block_validation_failed<E: ToString>(&mut self, _err: &E) {}

    fn on_message_received(&mut self, _message: &HotstuffMessage) {}

    fn on_error(&mut self, _err: &HotStuffError) {}

    fn on_pacemaker_height_changed(&mut self, _height: NodeHeight) {}

    fn on_leader_timeout(&mut self, _new_height: NodeHeight) {}

    fn on_needs_sync(&mut self, _local_height: NodeHeight, _remote_qc_height: NodeHeight) {}

    fn on_transaction_ready(&mut self, _tx_id: &TransactionId) {}

    fn on_transaction_batch_finalized(&mut self, _num_committed: usize, _num_aborted: usize) {}
}

/// Background worker that extracts and persists template metadata.
///
/// Runs on the blocking thread pool (via [`tokio::task::spawn_blocking`]) so that
/// synchronous WASM loading and RocksDB I/O do not block the async runtime.
///
/// At startup it backfills metadata for any templates that predate this deployment,
/// then enters a loop processing addresses sent by [`TemplateMetadataHooks`].
pub struct TemplateMetadataWorker {
    template_provider: StateStoreTemplateProvider<RocksDbStateStore<tari_ootle_p2p::PeerAddress>>,
    store: RocksDbStateStore<tari_ootle_p2p::PeerAddress>,
    rx_template_addresses: mpsc::UnboundedReceiver<TemplateAddress>,
}

impl TemplateMetadataWorker {
    pub fn new(
        template_provider: StateStoreTemplateProvider<RocksDbStateStore<tari_ootle_p2p::PeerAddress>>,
        store: RocksDbStateStore<tari_ootle_p2p::PeerAddress>,
        rx_template_addresses: mpsc::UnboundedReceiver<TemplateAddress>,
    ) -> Self {
        Self {
            template_provider,
            store,
            rx_template_addresses,
        }
    }

    /// Entry point for the blocking thread pool task.
    ///
    /// First backfills any templates that lack metadata, then processes new addresses
    /// from the channel until the sender is dropped (node shutdown).
    pub fn run(mut self) {
        self.backfill_missing();

        while let Some(address) = self.rx_template_addresses.blocking_recv() {
            self.write_template_metadata(&address);
        }
    }

    /// Backfills `TemplateMetadataCf` for all live template substates that currently lack a
    /// metadata entry. Covers templates published before this code was deployed.
    fn backfill_missing(&mut self) {
        let addresses = match self.store.scan_template_addresses_missing_metadata() {
            Ok(a) => a,
            Err(e) => {
                error!(
                    target: LOG_TARGET,
                    "Failed to scan template addresses for metadata backfill: {}", e
                );
                return;
            },
        };

        if addresses.is_empty() {
            return;
        }

        info!(
            target: LOG_TARGET,
            "Backfilling template metadata for {} template(s) that predate this deployment",
            addresses.len()
        );

        let mut succeeded = 0usize;
        for address in &addresses {
            self.write_template_metadata(address);
            succeeded += 1;
        }

        info!(
            target: LOG_TARGET,
            "Template metadata backfill complete: {}/{} entries written",
            succeeded,
            addresses.len()
        );
    }

    fn write_template_metadata(&mut self, address: &TemplateAddress) {
        // One store read for all on-chain fields (author, binary, epoch).
        // PublishedTemplate carries the raw WASM binary but we only hash it here; we do not parse it.
        let published = match self.store.get_template(address) {
            Ok(Some(t)) => t,
            Ok(None) => {
                warn!(
                    target: LOG_TARGET,
                    "Template {} not found in state store when trying to write metadata", address
                );
                return;
            },
            Err(e) => {
                error!(
                    target: LOG_TARGET,
                    "Failed to load template {} for metadata extraction: {}", address, e
                );
                return;
            },
        };

        // Template name comes from the parsed WASM module.
        // StateStoreTemplateProvider caches the parsed module in an LRU cache, so on cache hit
        // this path does not read from RocksDB again or re-parse WASM.
        let loaded = match self.template_provider.get_template(address) {
            Ok(Some(t)) => t,
            Ok(None) => {
                warn!(
                    target: LOG_TARGET,
                    "Parsed template {} not available when writing metadata", address
                );
                return;
            },
            Err(e) => {
                error!(
                    target: LOG_TARGET,
                    "Failed to get parsed template {} for name extraction: {}", address, e
                );
                return;
            },
        };

        let metadata = TemplateMetadata {
            template_name: loaded.template_name().to_string(),
            author_public_key: published.author,
            binary_hash: published.to_binary_hash(),
            at_epoch: published.at_epoch,
        };

        if let Err(e) = self.store.with_write_tx(
            |tx: &mut RocksDbStateStoreWriteTransaction<'_, _>| -> Result<(), StorageError> {
                tx.template_metadata_upsert(address, &metadata)
            },
        ) {
            error!(
                target: LOG_TARGET,
                "Failed to persist template metadata for {}: {}", address, e
            );
        }
    }
}

/// Composes two [`ConsensusHooks`] implementations into one, calling both in sequence.
///
/// Used to chain `PrometheusConsensusMetrics` (or `NoopHooks`) with `TemplateMetadataHooks`
/// without modifying the shared `crates/consensus` crate.
#[derive(Debug, Clone)]
pub struct CompositeHooks<A, B> {
    first: A,
    second: B,
}

impl<A: ConsensusHooks, B: ConsensusHooks> CompositeHooks<A, B> {
    pub fn new(first: A, second: B) -> Self {
        Self { first, second }
    }
}

impl<A: ConsensusHooks, B: ConsensusHooks> ConsensusHooks for CompositeHooks<A, B> {
    fn on_local_block_committed(&mut self, block: &ValidBlock) {
        self.first.on_local_block_committed(block);
        self.second.on_local_block_committed(block);
    }

    fn on_block_validation_failed<E: ToString>(&mut self, err: &E) {
        self.first.on_block_validation_failed(err);
        self.second.on_block_validation_failed(err);
    }

    fn on_message_received(&mut self, message: &HotstuffMessage) {
        self.first.on_message_received(message);
        self.second.on_message_received(message);
    }

    fn on_error(&mut self, err: &HotStuffError) {
        self.first.on_error(err);
        self.second.on_error(err);
    }

    fn on_pacemaker_height_changed(&mut self, height: NodeHeight) {
        self.first.on_pacemaker_height_changed(height);
        self.second.on_pacemaker_height_changed(height);
    }

    fn on_leader_timeout(&mut self, new_height: NodeHeight) {
        self.first.on_leader_timeout(new_height);
        self.second.on_leader_timeout(new_height);
    }

    fn on_needs_sync(&mut self, local_height: NodeHeight, remote_qc_height: NodeHeight) {
        self.first.on_needs_sync(local_height, remote_qc_height);
        self.second.on_needs_sync(local_height, remote_qc_height);
    }

    fn on_transaction_ready(&mut self, tx_id: &TransactionId) {
        self.first.on_transaction_ready(tx_id);
        self.second.on_transaction_ready(tx_id);
    }

    fn on_transaction_batch_finalized(&mut self, num_committed: usize, num_aborted: usize) {
        self.first.on_transaction_batch_finalized(num_committed, num_aborted);
        self.second.on_transaction_batch_finalized(num_committed, num_aborted);
    }
}
