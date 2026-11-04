//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use log::*;
use tari_consensus_types::{LastProposed, LeafBlock};
use tari_ootle_common_types::{Epoch, NodeHeight, optional::Optional};
use tari_ootle_storage::{
    StateStore,
    StorageError,
    consensus_models::{Block, BookkeepingModel},
};

use crate::{
    bounded_spawn::PerKeyBoundedSpawn,
    hotstuff::HotStuffError,
    messages::{CatchUpRequestMessage, HotstuffMessage, ProposalMessage},
    traits::{ConsensusSpec, OutboundMessaging},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_sync_request";

/// Number of catch-up requests served concurrently. Each one walks the block store and streams every block up
/// to our leaf, so the work this node does must not scale with the number of peers asking.
const MAX_CONCURRENT_SYNC_REQUESTS: usize = 5;
/// A peer only ever needs one response at a time, and holding it to one keeps the rest of the pool for its committee.
const MAX_SYNC_REQUESTS_PER_PEER: usize = 1;
/// A response streams every block up to our leaf, so it is allowed to be slow. Past this the peer is not keeping up
/// with what it asked for, and its permit is worth more to someone else.
const SYNC_RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);
/// Number of blocks read from the store at a time while serving a catch-up request.
const CATCH_UP_BATCH_SIZE: usize = 100;

#[derive(Debug)]
pub struct OnSyncRequest<TConsensusSpec: ConsensusSpec> {
    store: TConsensusSpec::StateStore,
    outbound_messaging: TConsensusSpec::OutboundMessaging,
    bounded_spawner: PerKeyBoundedSpawn<TConsensusSpec::Addr>,
}

impl<TConsensusSpec: ConsensusSpec> OnSyncRequest<TConsensusSpec> {
    pub fn new(store: TConsensusSpec::StateStore, outbound_messaging: TConsensusSpec::OutboundMessaging) -> Self {
        Self {
            store,
            outbound_messaging,
            bounded_spawner: PerKeyBoundedSpawn::new(
                MAX_CONCURRENT_SYNC_REQUESTS,
                MAX_SYNC_REQUESTS_PER_PEER,
                SYNC_RESPONSE_TIMEOUT,
            ),
        }
    }

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

        let outbound_messaging = self.outbound_messaging.clone();
        let store = self.store.clone();

        if self
            .bounded_spawner
            .try_spawn(from.clone(), {
                let from = from.clone();
                Self::handle_request_task(store, outbound_messaging, from, epoch, msg)
            })
            .is_err()
        {
            warn!(
                target: LOG_TARGET,
                "⚠️ No catch-up request slot available for {}, dropping the request",
                from
            );
        }
    }

    #[allow(clippy::too_many_lines)]
    async fn handle_request_task(
        store: TConsensusSpec::StateStore,
        mut outbound_messaging: TConsensusSpec::OutboundMessaging,
        from: TConsensusSpec::Addr,
        epoch: Epoch,
        msg: CatchUpRequestMessage,
    ) {
        let result = store.with_read_tx(|tx| {
            let mut leaf_block = LeafBlock::get(tx, epoch)?;
            // Include the block we last proposed if applicable.
            if let Some(last_proposed) = LastProposed::get(tx, epoch).optional()? &&
                last_proposed.epoch == leaf_block.epoch() &&
                last_proposed.height > leaf_block.height()
            {
                leaf_block = last_proposed.as_leaf_block();
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
                info!(
                    target: LOG_TARGET,
                    "This node is at height 0 so cannot return any sync blocks. Ignoring request"
                );
                return Ok(None);
            }

            if leaf_block.height() < msg.block_height {
                return Err(HotStuffError::InvalidSyncRequest {
                    details: format!(
                        "Received catch up request from {} for block {} but our leaf block is {}. Ignoring request.",
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

        let start_height = msg.block_height.max(NodeHeight(1));
        let batches = CatchUpBatches::new(&store, msg.epoch, start_height, leaf_block.height());
        for result in batches {
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
                    "No blocks found from height {} to {} for epoch {}",
                    start_height,
                    leaf_block.height(),
                    epoch
                );
                return;
            }

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

                let is_leaf = block.id() == leaf_block.block_id();
                let proposal = ProposalMessage {
                    block,
                    foreign_proposals: foreign_proposals.into_iter().map(|p| p.into_proposal()).collect(),
                };
                // The requester does not vote on catch-up blocks, because a later block certifies each ancestor of
                // our leaf. The leaf has no certificate yet, so it goes as a proposal: the requester votes on it if it
                // is still current when it gets there.
                let message = if is_leaf {
                    HotstuffMessage::new_proposal(proposal)
                } else {
                    HotstuffMessage::new_catch_up_sync_response(proposal)
                };

                if let Err(err) = outbound_messaging.send(from.clone(), message).await {
                    warn!(target: LOG_TARGET, "Error sending SyncResponse: {err}");
                    return;
                }
            }
        }
    }
}

/// The blocks of `epoch` a catch-up response sends, from a start height up to and including the leaf, read in
/// batches of at most [`CATCH_UP_BATCH_SIZE`] so that no single read transaction spans the whole range.
struct CatchUpBatches<'a, TStore> {
    store: &'a TStore,
    epoch: Epoch,
    next_height: NodeHeight,
    leaf_height: NodeHeight,
}

impl<'a, TStore> CatchUpBatches<'a, TStore> {
    fn new(store: &'a TStore, epoch: Epoch, start_height: NodeHeight, leaf_height: NodeHeight) -> Self {
        Self {
            store,
            epoch,
            next_height: start_height,
            leaf_height,
        }
    }
}

impl<TStore: StateStore> Iterator for CatchUpBatches<'_, TStore> {
    type Item = Result<Vec<Block>, StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_height > self.leaf_height {
            return None;
        }
        let result = self.store.with_read_tx(|tx| {
            Block::get_all_blocks_between(
                tx,
                self.epoch,
                self.next_height,
                self.leaf_height,
                false,
                CATCH_UP_BATCH_SIZE,
            )
        });
        // A failed or empty read ends the response, so the next call reads nothing
        self.next_height = match &result {
            Ok(blocks) => blocks.last().map(|block| block.height() + NodeHeight(1)),
            Err(_) => None,
        }
        .unwrap_or(self.leaf_height + NodeHeight(1));
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use tari_consensus_types::{ProposalCertificate, ShardGroupAccumulatedData};
    use tari_crypto::tari_utilities::epoch_time::EpochTime;
    use tari_ootle_common_types::{ExtraData, NumPreshards, ProtocolVersion, ShardGroup};
    use tari_ootle_storage::consensus_models::{BlockCommands, BlockHeader};
    use tari_ootle_transaction::Network;
    use tari_sidechain::{FixedHash, QuorumDecision};
    use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};
    use tari_template_lib_types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

    use super::*;
    use crate::traits::CertificateStore;

    const NETWORK: Network = Network::LocalNet;
    const NUM_PRESHARDS: NumPreshards = NumPreshards::P256;

    /// A block extending `parent`. It carries a signature, since a catch-up response skips unsigned (dummy) blocks;
    /// the store does not check it.
    fn child(parent: &Block) -> Block {
        let justify = ProposalCertificate::new(
            *parent.id().hash(),
            *parent.id(),
            parent.height(),
            parent.epoch(),
            ShardGroup::all_shards(NUM_PRESHARDS),
            ProposalCertificate::genesis_signatures(),
            QuorumDecision::Accept,
        );
        let commands = BlockCommands::empty();
        let mut header = BlockHeader::create_unsigned(
            NETWORK,
            ProtocolVersion::V0,
            *parent.id(),
            justify.calculate_id(),
            None,
            parent.height() + NodeHeight(1),
            Epoch::zero(),
            ShardGroup::all_shards(NUM_PRESHARDS),
            RistrettoPublicKeyBytes::default(),
            FixedHash::zero(),
            &commands,
            0,
            EpochTime::now().as_u64(),
            FixedHash::zero(),
            ShardGroupAccumulatedData::default(),
            ExtraData::new(),
        )
        .unwrap();
        header.set_signature(SchnorrSignatureBytes::zero());
        Block::new(header, justify, commands, None)
    }

    /// A store holding a chain of blocks at heights one to `leaf_height` in epoch zero.
    fn store_with_chain(leaf_height: u64) -> (RocksDbStateStore<String>, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let store = RocksDbStateStore::<String>::open(tmp.path(), DatabaseOptions::default()).unwrap();
        let mut block = Block::zero_block(NETWORK, NUM_PRESHARDS);
        store
            .with_write_tx(|tx| {
                block.justify().save(tx)?;
                block.insert(tx)?;
                for _ in 0..leaf_height {
                    block = child(&block);
                    block.justify().save(tx)?;
                    block.insert(tx)?;
                }
                Ok::<_, StorageError>(())
            })
            .unwrap();
        (store, tmp)
    }

    fn batch_heights(store: &RocksDbStateStore<String>, start: u64, leaf: u64) -> Vec<Vec<u64>> {
        CatchUpBatches::new(store, Epoch::zero(), NodeHeight(start), NodeHeight(leaf))
            .map(|batch| batch.unwrap().iter().map(|block| block.height().as_u64()).collect())
            .collect()
    }

    #[test]
    fn a_leaf_just_past_a_full_batch_is_sent_in_a_batch_of_its_own() {
        let leaf = CATCH_UP_BATCH_SIZE as u64 + 1;
        let (store, _tmp) = store_with_chain(leaf);

        let batches = batch_heights(&store, 1, leaf);

        assert_eq!(batches.len(), 2, "{batches:?}");
        assert_eq!(batches[0], (1..leaf).collect::<Vec<_>>());
        assert_eq!(batches[1], [leaf]);
    }

    #[test]
    fn a_request_at_the_leaf_height_sends_the_leaf() {
        let (store, _tmp) = store_with_chain(5);

        assert_eq!(batch_heights(&store, 5, 5), [[5]]);
    }
}
