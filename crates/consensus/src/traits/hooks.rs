//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::NodeHeight;
use tari_ootle_storage::consensus_models::{Block, ValidBlock};
use tari_ootle_transaction::TransactionId;

use crate::{hotstuff::HotStuffError, messages::HotstuffMessage};

pub trait ConsensusHooks {
    fn on_local_block_committed(&mut self, block: &ValidBlock);

    /// Called with the ancestor blocks whose substates have just been written to the state store.
    fn on_blocks_committed(&mut self, _committed_blocks: &[Block]) {}

    fn on_block_validation_failed<E: ToString>(&mut self, err: &E);
    fn on_message_received(&mut self, message: &HotstuffMessage);
    fn on_error(&mut self, err: &HotStuffError);
    fn on_pacemaker_height_changed(&mut self, height: NodeHeight);
    fn on_leader_timeout(&mut self, new_height: NodeHeight);

    fn on_needs_sync(&mut self, local_height: NodeHeight, remote_qc_height: NodeHeight);

    fn on_transaction_ready(&mut self, tx_id: &TransactionId);
    fn on_transaction_batch_finalized(&mut self, num_committed: usize, num_aborted: usize);
}

#[derive(Debug, Clone)]
pub struct OptionalHooks<T> {
    inner: Option<T>,
}

impl<T> OptionalHooks<T> {
    pub fn enabled(inner: T) -> Self {
        Self { inner: Some(inner) }
    }

    pub fn disabled() -> Self {
        Self { inner: None }
    }
}

impl<T: ConsensusHooks> ConsensusHooks for OptionalHooks<T> {
    fn on_local_block_committed(&mut self, block: &ValidBlock) {
        if let Some(inner) = self.inner.as_mut() {
            inner.on_local_block_committed(block);
        }
    }

    fn on_blocks_committed(&mut self, committed_blocks: &[Block]) {
        if let Some(inner) = self.inner.as_mut() {
            inner.on_blocks_committed(committed_blocks);
        }
    }

    fn on_block_validation_failed<E: ToString>(&mut self, err: &E) {
        if let Some(inner) = self.inner.as_mut() {
            inner.on_block_validation_failed(err);
        }
    }

    fn on_message_received(&mut self, message: &HotstuffMessage) {
        if let Some(inner) = self.inner.as_mut() {
            inner.on_message_received(message);
        }
    }

    fn on_error(&mut self, err: &HotStuffError) {
        if let Some(inner) = self.inner.as_mut() {
            inner.on_error(err);
        }
    }

    fn on_pacemaker_height_changed(&mut self, height: NodeHeight) {
        if let Some(inner) = self.inner.as_mut() {
            inner.on_pacemaker_height_changed(height);
        }
    }

    fn on_leader_timeout(&mut self, new_height: NodeHeight) {
        if let Some(inner) = self.inner.as_mut() {
            inner.on_leader_timeout(new_height);
        }
    }

    fn on_needs_sync(&mut self, local_height: NodeHeight, remote_qc_height: NodeHeight) {
        if let Some(inner) = self.inner.as_mut() {
            inner.on_needs_sync(local_height, remote_qc_height);
        }
    }

    fn on_transaction_ready(&mut self, tx_id: &TransactionId) {
        if let Some(inner) = self.inner.as_mut() {
            inner.on_transaction_ready(tx_id);
        }
    }

    fn on_transaction_batch_finalized(&mut self, num_committed: usize, num_aborted: usize) {
        if let Some(inner) = self.inner.as_mut() {
            inner.on_transaction_batch_finalized(num_committed, num_aborted);
        }
    }
}

impl<T> From<T> for OptionalHooks<T> {
    fn from(inner: T) -> Self {
        Self { inner: Some(inner) }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopHooks;

impl ConsensusHooks for NoopHooks {
    fn on_local_block_committed(&mut self, _block: &ValidBlock) {}

    fn on_block_validation_failed<E: ToString>(&mut self, _: &E) {}

    fn on_message_received(&mut self, _message: &HotstuffMessage) {}

    fn on_error(&mut self, _err: &HotStuffError) {}

    fn on_pacemaker_height_changed(&mut self, _: NodeHeight) {}

    fn on_leader_timeout(&mut self, _new_height: NodeHeight) {}

    fn on_needs_sync(&mut self, _local_height: NodeHeight, _remote_qc_height: NodeHeight) {}

    fn on_transaction_ready(&mut self, _tx_id: &TransactionId) {}

    fn on_transaction_batch_finalized(&mut self, _num_committed: usize, _num_aborted: usize) {}
}

/// Composes two [`ConsensusHooks`] implementations into one, calling both in sequence.
///
/// Used to chain `PrometheusConsensusMetrics` (or `NoopHooks`) with `TemplateMetadataHooks`
/// without modifying the shared `crates/consensus` crate.
#[derive(Debug, Clone)]
pub struct CompositeHook<A, B> {
    first: A,
    second: B,
}

impl<A: ConsensusHooks, B: ConsensusHooks> CompositeHook<A, B> {
    pub fn new(first: A, second: B) -> Self {
        Self { first, second }
    }
}

impl<A: ConsensusHooks, B: ConsensusHooks> ConsensusHooks for CompositeHook<A, B> {
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
