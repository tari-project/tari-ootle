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
use tari_ootle_storage::{
    StateStore,
    StorageError,
    consensus_models::{Block, ValidBlock},
};
use tari_ootle_transaction::TransactionId;
use tari_state_store_rocksdb::{RocksDbStateStore, writer::RocksDbStateStoreWriteTransaction};
use tari_template_lib::types::TemplateAddress;
use tokio::sync::mpsc;

use crate::state_store_template_provider::StateStoreTemplateProvider;

const LOG_TARGET: &str = "tari::validator::consensus::template_metadata_hooks";

/// Consensus hook that forwards newly committed template addresses to a background worker.
///
/// All methods are non-blocking: `on_local_block_committed` collects all template addresses
/// from the committed blocks into a `Vec` and sends them as a single batch through an unbounded
/// channel to [`TemplateMetadataWorker`], which performs the expensive WASM loading and RocksDB
/// writes off the consensus thread.
///
/// Template addresses are extracted from `committed_blocks` (not from `valid_block`), because
/// only the committed blocks have had their substates written to the state store by the time
/// this hook fires.
#[derive(Clone)]
pub struct TemplateMetadataHooks {
    tx_template_addresses: mpsc::UnboundedSender<Vec<TemplateAddress>>,
}

impl TemplateMetadataHooks {
    pub fn new(tx_template_addresses: mpsc::UnboundedSender<Vec<TemplateAddress>>) -> Self {
        Self { tx_template_addresses }
    }
}

impl ConsensusHooks for TemplateMetadataHooks {
    fn on_local_block_committed(&mut self, _block: &ValidBlock) {}

    fn on_blocks_committed(&mut self, committed_blocks: &[Block]) {
        // Extract templates from committed blocks — their substates are now in the state store.
        let template_addresses: Vec<TemplateAddress> = committed_blocks
            .iter()
            .flat_map(|b| b.commands())
            .filter_map(|c| c.committing())
            .flat_map(|a| a.evidence.all_outputs_iter())
            .filter(|(_, id, _)| id.is_template())
            .filter_map(|(_, id, _)| id.as_template())
            .map(|published_addr| published_addr.as_template_address())
            .collect();

        if template_addresses.is_empty() {
            return;
        }

        // Non-blocking: if the receiver has dropped (shutdown), silently discard.
        let _unused = self.tx_template_addresses.send(template_addresses);
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
/// then enters a loop processing batches sent by [`TemplateMetadataHooks`]. Each batch
/// corresponds to one committed block and is written in a single RocksDB transaction.
pub struct TemplateMetadataWorker {
    template_provider: StateStoreTemplateProvider<RocksDbStateStore<tari_ootle_p2p::PeerAddress>>,
    store: RocksDbStateStore<tari_ootle_p2p::PeerAddress>,
    rx_template_addresses: mpsc::UnboundedReceiver<Vec<TemplateAddress>>,
}

impl TemplateMetadataWorker {
    pub fn new(
        template_provider: StateStoreTemplateProvider<RocksDbStateStore<tari_ootle_p2p::PeerAddress>>,
        store: RocksDbStateStore<tari_ootle_p2p::PeerAddress>,
        rx_template_addresses: mpsc::UnboundedReceiver<Vec<TemplateAddress>>,
    ) -> Self {
        Self {
            template_provider,
            store,
            rx_template_addresses,
        }
    }

    /// Entry point for the blocking thread pool task.
    ///
    /// First backfills any templates that lack metadata, then processes batches of addresses
    /// from the channel until the sender is dropped (node shutdown).
    pub fn run(mut self) {
        self.backfill_missing();

        while let Some(addresses) = self.rx_template_addresses.blocking_recv() {
            self.write_metadata_batch(&addresses);
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

        let before = addresses.len();
        self.write_metadata_batch(&addresses);

        info!(
            target: LOG_TARGET,
            "Template metadata backfill complete: {} entries written",
            before
        );
    }

    /// Prepares and writes metadata for all addresses in `batch` using a single write transaction.
    ///
    /// Addresses for which metadata cannot be prepared (template not found, WASM load failure)
    /// are skipped with an error log; the remaining entries are still written.
    fn write_metadata_batch(&mut self, addresses: &[TemplateAddress]) {
        let prepared: Vec<(TemplateAddress, TemplateMetadata)> = addresses
            .iter()
            .filter_map(|address| self.prepare_metadata(address).map(|m| (*address, m)))
            .collect();

        if prepared.is_empty() {
            return;
        }

        if let Err(e) = self.store.with_write_tx(
            |tx: &mut RocksDbStateStoreWriteTransaction<'_, _>| -> Result<(), StorageError> {
                for (address, metadata) in &prepared {
                    tx.template_metadata_upsert(address, metadata)?;
                }
                Ok(())
            },
        ) {
            error!(
                target: LOG_TARGET,
                "Failed to persist template metadata batch ({} entries): {}", prepared.len(), e
            );
        }
    }

    /// Loads on-chain fields and the WASM-derived name for one template address.
    ///
    /// Returns `None` and logs an error/warning if either the state store or the template
    /// provider cannot supply the required data.
    fn prepare_metadata(&mut self, address: &TemplateAddress) -> Option<TemplateMetadata> {
        // One store read for all on-chain fields (author, binary, epoch).
        // PublishedTemplate carries the raw WASM binary but we only hash it; we do not parse it.
        let published = match self.store.get_template(address) {
            Ok(Some(t)) => t,
            Ok(None) => {
                warn!(
                    target: LOG_TARGET,
                    "Template {} not found in state store when trying to write metadata", address
                );
                return None;
            },
            Err(e) => {
                error!(
                    target: LOG_TARGET,
                    "Failed to load template {} for metadata extraction: {}", address, e
                );
                return None;
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
                return None;
            },
            Err(e) => {
                error!(
                    target: LOG_TARGET,
                    "Failed to get parsed template {} for name extraction: {}", address, e
                );
                return None;
            },
        };

        Some(TemplateMetadata {
            template_name: loaded.template_name().to_string(),
            author_public_key: published.author,
            binary_hash: published.to_binary_hash(),
            at_epoch: published.at_epoch,
        })
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

    fn on_blocks_committed(&mut self, committed_blocks: &[Block]) {
        self.first.on_blocks_committed(committed_blocks);
        self.second.on_blocks_committed(committed_blocks);
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
