//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Consensus hook that writes lightweight template metadata to a dedicated RocksDB column family
//! whenever a template substate is committed to a block.
//!
//! The metadata (name, author, hash, epoch) is later served to indexers that set the
//! `TEMPLATE_METADATA` flag in their `sync_state` request, allowing catalogue discovery
//! without transmitting full WASM binaries.

use log::*;
use tari_consensus::{hotstuff::HotStuffError, messages::HotstuffMessage, traits::hooks::ConsensusHooks};
use tari_engine_types::published_template::{PublishedTemplateAddress, TemplateMetadata};
use tari_ootle_common_types::NodeHeight;
use tari_ootle_storage::{
    StorageError,
    consensus_models::{Block, ValidBlock},
};
use tari_ootle_transaction::TransactionId;
use tari_state_store_rocksdb::RocksDbStateStore;
use tokio::sync::mpsc;

use crate::state_store_template_provider::{StateStoreTemplateProvider, build_template_metadata};

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
    tx_template_addresses: mpsc::UnboundedSender<PublishedTemplateAddress>,
}

impl TemplateMetadataHooks {
    pub fn new(tx_template_addresses: mpsc::UnboundedSender<PublishedTemplateAddress>) -> Self {
        Self { tx_template_addresses }
    }
}

impl ConsensusHooks for TemplateMetadataHooks {
    fn on_local_block_committed(&mut self, _block: &ValidBlock) {}

    fn on_blocks_committed(&mut self, committed_blocks: &[Block]) {
        // Extract templates from committed blocks — their substates are now in the state store.
        let template_addresses = committed_blocks
            .iter()
            .flat_map(|b| b.commands())
            .filter_map(|c| c.committing())
            .flat_map(|a| a.evidence.all_outputs_iter())
            .filter_map(|(_, id, _)| id.as_template());

        // Non-blocking: if the receiver has dropped (shutdown), silently discard.
        for addr in template_addresses {
            if self.tx_template_addresses.send(addr).is_err() {
                warn!(target: LOG_TARGET, "Template metadata hook receiver dropped");
                return;
            }
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
pub struct TemplateMetadataWorker {
    template_provider: StateStoreTemplateProvider<RocksDbStateStore<tari_ootle_p2p::PeerAddress>>,
    store: RocksDbStateStore<tari_ootle_p2p::PeerAddress>,
    rx_template_addresses: mpsc::UnboundedReceiver<PublishedTemplateAddress>,
}

impl TemplateMetadataWorker {
    pub fn new(
        template_provider: StateStoreTemplateProvider<RocksDbStateStore<tari_ootle_p2p::PeerAddress>>,
        store: RocksDbStateStore<tari_ootle_p2p::PeerAddress>,
        rx_template_addresses: mpsc::UnboundedReceiver<PublishedTemplateAddress>,
    ) -> Self {
        Self {
            template_provider,
            store,
            rx_template_addresses,
        }
    }

    /// Entry point for the blocking thread pool task.
    ///
    /// Processes batches of addresses from the channel until the sender is dropped (node shutdown).
    pub async fn run(mut self) {
        const BUFFER_LIMIT: usize = 100;
        let mut buffer = Vec::with_capacity(10);
        loop {
            let num_added = self.rx_template_addresses.recv_many(&mut buffer, BUFFER_LIMIT).await;
            if num_added == 0 {
                debug!(target: LOG_TARGET, "No more template addresses to process, shutting down metadata worker");
                break;
            }
            if let Err(err) = self.write_metadata_batch(&buffer) {
                error!(
                    target: LOG_TARGET,
                    "Failed to persist template metadata batch ({} entries): {}", num_added, err
                );
            }
        }
    }

    /// Prepares and writes metadata for all addresses in `batch` using a single write transaction.
    ///
    /// Addresses for which metadata cannot be prepared (template not found, WASM load failure)
    /// are skipped with an error log; the remaining entries are still written.
    fn write_metadata_batch(&mut self, addresses: &[PublishedTemplateAddress]) -> Result<(), StorageError> {
        let prepared = addresses
            .iter()
            .filter_map(|address| self.prepare_metadata(address).map(|m| (*address, m)))
            .collect::<Vec<_>>();

        if prepared.is_empty() {
            return Ok(());
        }

        // TODO: remove this file - template metadata is now derived from the substate value
        let _ = prepared;
        Ok(())
    }

    /// Loads on-chain fields and the WASM-derived name for one template address.
    ///
    /// Returns `None` and logs an error/warning if either the state store or the template
    /// provider cannot supply the required data.
    fn prepare_metadata(&mut self, address: &PublishedTemplateAddress) -> Option<TemplateMetadata> {
        match build_template_metadata(&self.template_provider, &address.as_template_address()) {
            Ok(Some(m)) => Some(m),
            Ok(None) => {
                warn!(
                    target: LOG_TARGET,
                    "Template {} not found when trying to write metadata", address
                );
                None
            },
            Err(e) => {
                error!(
                    target: LOG_TARGET,
                    "Failed to prepare metadata for template {}: {}", address, e
                );
                None
            },
        }
    }
}
