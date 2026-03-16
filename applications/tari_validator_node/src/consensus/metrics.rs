//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::atomic::AtomicU64;

use prometheus_client::{
    metrics::{counter::Counter, gauge::Gauge},
    registry::Registry,
};
use tari_consensus::{hotstuff::HotStuffError, messages::HotstuffMessage, traits::hooks::ConsensusHooks};
use tari_ootle_common_types::NodeHeight;
use tari_ootle_storage::consensus_models::ValidBlock;
use tari_ootle_transaction::TransactionId;

use crate::metrics::CollectorRegister;

type UnsignedGauge = Gauge<u64, AtomicU64>;

#[derive(Debug, Clone)]
pub struct PrometheusConsensusMetrics {
    blocks_committed: Counter,
    blocks_validation_failed: Counter,
    commit_height: UnsignedGauge,

    commands_count: UnsignedGauge,
    published_templates_count: Counter,

    messages_received: Counter,

    errors: Counter,

    pacemaker_height: UnsignedGauge,
    pacemaker_leader_failures: Counter,
    needs_sync: Counter,

    transactions_ready_for_consensus: Counter,
    transactions_finalized_committed: Counter,
    transactions_finalized_aborted: Counter,
}

impl PrometheusConsensusMetrics {
    pub fn register(registry: &mut Registry) -> Self {
        let registry = registry.sub_registry_with_prefix("consensus");
        Self {
            blocks_committed: Counter::default().register_at(
                "blocks_committed",
                "Number of committed blocks",
                registry,
            ),
            commit_height: UnsignedGauge::default().register_at(
                "commit_height",
                "Current block commit height",
                registry,
            ),
            commands_count: UnsignedGauge::default().register_at("num_commands", "Number of commands added", registry),
            published_templates_count: Counter::default().register_at(
                "published_templates_count",
                "Number of templates published",
                registry,
            ),
            messages_received: Counter::default().register_at(
                "messages_received",
                "Number of messages received",
                registry,
            ),
            errors: Counter::default().register_at("errors", "Number of errors", registry),
            pacemaker_height: UnsignedGauge::default().register_at(
                "pacemaker_height",
                "Current pacemaker height",
                registry,
            ),
            pacemaker_leader_failures: Counter::default().register_at(
                "leader_failures",
                "Number of leader failures",
                registry,
            ),
            blocks_validation_failed: Counter::default().register_at(
                "block_validation_failed",
                "Number of block validation failures",
                registry,
            ),
            needs_sync: Counter::default().register_at(
                "needs_sync",
                "Number of times consensus needs to sync",
                registry,
            ),
            transactions_ready_for_consensus: Counter::default().register_at(
                "transaction_ready_for_consensus",
                "Number of transactions ready for consensus",
                registry,
            ),
            transactions_finalized_committed: Counter::default().register_at(
                "transaction_finalized_committed",
                "Number of committed transactions",
                registry,
            ),
            transactions_finalized_aborted: Counter::default().register_at(
                "transaction_finalized_aborted",
                "Number of aborted transactions",
                registry,
            ),
        }
    }
}

impl ConsensusHooks for PrometheusConsensusMetrics {
    fn on_local_block_committed(&mut self, block: &ValidBlock) {
        self.blocks_committed.inc();
        self.commit_height.set(block.block().height().as_u64());
        self.commands_count.inc_by(block.block().commands().len() as u64);

        // Count the number of template outputs created by this block
        let num_templates_committed = block
            .block()
            .commands()
            .iter()
            .filter_map(|c| c.committing())
            .map(|a| {
                a.evidence
                    .iter()
                    .map(|(_, e)| e.outputs().iter().map(|(id, _)| id.is_template()).count() as u64)
                    .sum::<u64>()
            })
            .sum();
        self.published_templates_count.inc_by(num_templates_committed);
    }

    fn on_block_validation_failed<E: ToString>(&mut self, _err: &E) {
        self.blocks_validation_failed.inc();
    }

    fn on_message_received(&mut self, _message: &HotstuffMessage) {
        self.messages_received.inc();
    }

    fn on_error(&mut self, _err: &HotStuffError) {
        self.errors.inc();
    }

    fn on_pacemaker_height_changed(&mut self, height: NodeHeight) {
        self.pacemaker_height.set(height.as_u64());
    }

    fn on_leader_timeout(&mut self, _new_height: NodeHeight) {
        self.pacemaker_leader_failures.inc();
    }

    fn on_needs_sync(&mut self, _local_height: NodeHeight, _remote_qc_height: NodeHeight) {
        self.needs_sync.inc();
    }

    fn on_transaction_ready(&mut self, _tx_id: &TransactionId) {
        self.transactions_ready_for_consensus.inc();
    }

    fn on_transaction_batch_finalized(&mut self, num_committed: usize, num_aborted: usize) {
        self.transactions_finalized_committed.inc_by(num_committed as u64);
        self.transactions_finalized_aborted.inc_by(num_aborted as u64);
    }
}
