//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus::{hotstuff::HotStuffError, messages::HotstuffMessage, traits::hooks::ConsensusHooks};
use tari_consensus_types::BlockId;
use tari_ootle_common_types::NodeHeight;
use tari_ootle_storage::consensus_models::{Block, NoVoteReason, ValidBlock};
use tari_ootle_transaction::TransactionId;

use crate::template_prewarm::TemplatePrewarmer;

/// Queues a newly published template for compilation once its substate is in the state store.
///
/// The commit is the trigger rather than the publishing execution: the address a template will hold
/// is only settled when its substate commits, and a transaction that publishes one may still abort.
/// From the commit on, any transaction may call the template, and this node has its source but not
/// its compiled form.
#[derive(Debug, Clone)]
pub struct TemplatePrewarmHooks {
    prewarmer: TemplatePrewarmer,
}

impl TemplatePrewarmHooks {
    pub fn new(prewarmer: TemplatePrewarmer) -> Self {
        Self { prewarmer }
    }
}

impl ConsensusHooks for TemplatePrewarmHooks {
    fn on_blocks_committed(&mut self, committed_blocks: &[Block]) {
        let templates = committed_blocks
            .iter()
            .flat_map(|block| block.commands())
            .filter_map(|command| command.committing())
            .flat_map(|atom| atom.evidence.all_outputs_iter())
            .filter_map(|(_, substate_id, _)| substate_id.as_template());

        for address in templates {
            self.prewarmer.prewarm_template(address.as_template_address());
        }
    }

    fn on_local_block_committed(&mut self, _block: &ValidBlock) {}

    fn on_block_validation_failed<E: ToString>(&mut self, _err: &E) {}

    fn on_message_received(&mut self, _message: &HotstuffMessage) {}

    fn on_error(&mut self, _err: &HotStuffError) {}

    fn on_pacemaker_height_changed(&mut self, _height: NodeHeight) {}

    fn on_leader_timeout(&mut self, _new_height: NodeHeight) {}

    fn on_needs_sync(&mut self, _local_height: NodeHeight, _remote_qc_height: NodeHeight) {}

    fn on_no_vote(&mut self, _block_id: &BlockId, _reason: &NoVoteReason) {}

    fn on_transaction_ready(&mut self, _tx_id: &TransactionId) {}

    fn on_transaction_batch_finalized(&mut self, _num_committed: usize, _num_aborted: usize) {}
}
