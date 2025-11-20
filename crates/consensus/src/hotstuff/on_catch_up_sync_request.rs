//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_consensus_types::{LastProposed, LeafBlock};
use tari_ootle_common_types::{optional::Optional, Epoch, NodeHeight};
use tari_ootle_storage::{
    consensus_models::{Block, BookkeepingModel},
    StateStore,
};
use tokio::task;

use crate::{
    hotstuff::HotStuffError,
    messages::{CatchUpRequestMessage, HotstuffMessage, ProposalMessage},
    traits::{ConsensusSpec, OutboundMessaging},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_sync_request";

#[derive(Debug)]
pub struct OnSyncRequest<TConsensusSpec: ConsensusSpec> {
    store: TConsensusSpec::StateStore,
    outbound_messaging: TConsensusSpec::OutboundMessaging,
}

impl<TConsensusSpec: ConsensusSpec> OnSyncRequest<TConsensusSpec> {
    pub fn new(store: TConsensusSpec::StateStore, outbound_messaging: TConsensusSpec::OutboundMessaging) -> Self {
        Self {
            store,
            outbound_messaging,
        }
    }

    #[allow(clippy::too_many_lines)]
    pub fn handle(&self, from: TConsensusSpec::Addr, epoch: Epoch, msg: CatchUpRequestMessage) {
        if msg.epoch != epoch {
            warn!(
                target: LOG_TARGET,
                "Received SyncRequest from {} for epoch {} but our epoch is {}. Ignoring request.",
                from,
                msg.epoch,
                epoch
            );
            return;
        }

        let mut outbound_messaging = self.outbound_messaging.clone();
        let store = self.store.clone();

        task::spawn(async move {
            let result = store.with_read_tx(|tx| {
                let mut leaf_block = LeafBlock::get(tx, epoch)?;
                // Include the block we last proposed if applicable.
                if let Some(last_proposed) = LastProposed::get(tx, epoch).optional()? {
                    if last_proposed.epoch == leaf_block.epoch() && last_proposed.height > leaf_block.height() {
                        leaf_block = last_proposed.as_leaf_block();
                    }
                }

                if leaf_block.epoch() != msg.epoch {
                    info!(
                        target: LOG_TARGET,
                        "Received catch up request from {} for epoch {} but our leaf block is {}. Ignoring request.",
                        from,
                        msg.epoch,
                        leaf_block
                    );
                    return Ok(None);
                }

                if leaf_block.height.is_zero() {
                    info!(target: LOG_TARGET, "This node is at height 0 so cannot return any sync blocks. Ignoring request");
                    return Ok(None);
                }

                if leaf_block.height() < msg.block_height {
                    return Err(HotStuffError::InvalidSyncRequest {
                        details: format!(
                            "Received catch up request from {} for block {} but our leaf block is {}. Ignoring \
                             request.",
                            from, msg.block_height, leaf_block
                        ),
                    });
                }

                info!(
                    target: LOG_TARGET,
                    "🌐 Received catch up request from {} from block {} to {}",
                    from,
                    msg.block_height,
                    leaf_block
                );
                Ok(Some(leaf_block))
            });

            let leaf_block = match result {
                Ok(Some(leaf_block)) => leaf_block,
                Ok(None) => {
                    return;
                },
                Err(err) => {
                    warn!(target: LOG_TARGET, "Failed to process sync request: {}", err);
                    return;
                },
            };

            let mut start_height = msg.block_height.max(NodeHeight(1));
            while start_height < leaf_block.height() {
                let result = store.with_read_tx(|tx| {
                    Block::get_all_blocks_between(tx, msg.epoch, start_height, leaf_block.height(), false, 100)
                });

                let blocks = match result {
                    Ok(blocks) => blocks,
                    Err(err) => {
                        warn!(target: LOG_TARGET, "Failed to fetch blocks for catch-up request: {}", err);
                        return;
                    },
                };

                if blocks.is_empty() {
                    warn!(
                        target: LOG_TARGET,
                        "No blocks found between heights {} and {} for epoch {}",
                        start_height,
                        leaf_block.height(),
                        epoch
                    );
                    return;
                }
                start_height = blocks
                    .last()
                    .map(|b| b.height() + NodeHeight(1))
                    .unwrap_or(leaf_block.height());

                info!(
                    target: LOG_TARGET,
                    "🌐 Sending {} block(s) ({} to {}) to {}",
                    blocks.len(),
                    blocks.first().map(|b| b.height()).unwrap_or_default(),
                    blocks.last().map(|b| b.height()).unwrap_or_default(),
                    from
                );

                for block in blocks {
                    // TODO(perf): O(n) queries
                    let foreign_proposals = match store.with_read_tx(|tx| block.get_foreign_proposals(tx)) {
                        Ok(foreign_proposals) => foreign_proposals,
                        Err(err) => {
                            warn!(target: LOG_TARGET, "Failed to fetch foreign proposals for block {}: {}", block, err);
                            return;
                        },
                    };

                    debug!(
                        target: LOG_TARGET,
                        "🌐 Sending block {} to {}",
                        block,
                        from
                    );

                    if let Err(err) = outbound_messaging
                        .send(
                            from.clone(),
                            HotstuffMessage::new_proposal(ProposalMessage {
                                block,
                                foreign_proposals: foreign_proposals.into_iter().map(|p| p.into_proposal()).collect(),
                            }),
                        )
                        .await
                    {
                        warn!(target: LOG_TARGET, "Error sending SyncResponse: {err}");
                        return;
                    }
                }
            }
        });
    }
}
