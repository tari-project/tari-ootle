//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, time::Duration};

use log::*;
use tari_consensus_types::{LastProposed, LeafBlock};
use tari_ootle_common_types::{Epoch, NodeHeight, optional::Optional};
use tari_ootle_storage::{
    StateStore,
    StorageError,
    consensus_models::{Block, BookkeepingModel, TransactionRecord},
};

use crate::{
    bounded_spawn::PerKeyBoundedSpawn,
    hotstuff::HotStuffError,
    messages::{
        CatchUpPageMessage,
        CatchUpRequestMessage,
        EncodedTransaction,
        HotstuffMessage,
        MAX_CATCH_UP_PAGE_BLOCKS,
        MAX_REQUESTED_TRANSACTIONS,
        ProposalMessage,
    },
    traits::{ConsensusSpec, OutboundMessaging},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_sync_request";

/// Number of catch-up requests served concurrently. Each one reads a page of blocks from the store, so the work this
/// node does must not scale with the number of peers asking.
const MAX_CONCURRENT_SYNC_REQUESTS: usize = 5;
/// A requester has one page request outstanding at a time, and holding it to one keeps the rest of the pool for its
/// committee.
const MAX_SYNC_REQUESTS_PER_PEER: usize = 1;
/// Reading and sending one page takes well under this. Past it the permit is worth more to someone else.
const SYNC_RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);
/// Number of blocks read from the store at a time while filling a catch-up page.
const CATCH_UP_BATCH_SIZE: usize = 100;
/// What a transaction adds to a page beyond its encoded bytes: the field's tag and length prefix.
const TRANSACTION_FRAMING_BYTES: usize = 8;

#[derive(Debug)]
pub struct OnSyncRequest<TConsensusSpec: ConsensusSpec> {
    store: TConsensusSpec::StateStore,
    outbound_messaging: TConsensusSpec::OutboundMessaging,
    bounded_spawner: PerKeyBoundedSpawn<TConsensusSpec::Addr>,
    page_bytes: usize,
}

impl<TConsensusSpec: ConsensusSpec> OnSyncRequest<TConsensusSpec> {
    pub fn new(
        store: TConsensusSpec::StateStore,
        outbound_messaging: TConsensusSpec::OutboundMessaging,
        page_bytes: usize,
    ) -> Self {
        Self {
            store,
            outbound_messaging,
            bounded_spawner: PerKeyBoundedSpawn::new(
                MAX_CONCURRENT_SYNC_REQUESTS,
                MAX_SYNC_REQUESTS_PER_PEER,
                SYNC_RESPONSE_TIMEOUT,
            ),
            page_bytes,
        }
    }

    /// Answers `msg` with one page. A request beyond the pool's or the peer's allowance is dropped, and the requester
    /// asks again when its page does not arrive.
    pub fn handle(&self, from: TConsensusSpec::Addr, epoch: Epoch, msg: CatchUpRequestMessage) {
        let outbound_messaging = self.outbound_messaging.clone();
        let store = self.store.clone();
        let page_bytes = self.page_bytes;

        if self
            .bounded_spawner
            .try_spawn(from.clone(), {
                let from = from.clone();
                async move {
                    let page = match read_page(&store, &outbound_messaging, &from, epoch, &msg, page_bytes) {
                        Ok(page) => page,
                        Err(err) => {
                            warn!(target: LOG_TARGET, "Failed to read a catch-up page for {from}: {err}");
                            CatchUpPageMessage::empty(msg.request_id, msg.epoch)
                        },
                    };
                    let mut outbound_messaging = outbound_messaging;
                    if let Err(err) = outbound_messaging
                        .send(from.clone(), HotstuffMessage::CatchUpPage(Box::new(page)))
                        .await
                    {
                        warn!(target: LOG_TARGET, "Error sending catch-up page to {from}: {err}");
                    }
                }
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
}

/// Reads the page `msg` asks for: the blocks of the requested epoch from `msg.from_height`, up to and including
/// our leaf, filled up to `page_bytes` with at least one block, and the transactions those blocks commit to that
/// still fit. A request this node cannot serve gets a page without blocks.
fn read_page<TStore: StateStore, TOutbound: OutboundMessaging>(
    store: &TStore,
    outbound_messaging: &TOutbound,
    from: &impl std::fmt::Display,
    epoch: Epoch,
    msg: &CatchUpRequestMessage,
    page_bytes: usize,
) -> Result<CatchUpPageMessage, HotStuffError> {
    let mut page = CatchUpPageMessage::empty(msg.request_id, msg.epoch);
    if msg.epoch != epoch {
        info!(
            target: LOG_TARGET,
            "Received catch up request from {from} for epoch {} but our epoch is {epoch}. Sending an empty page.",
            msg.epoch,
        );
        return Ok(page);
    }

    let leaf_block = store.with_read_tx(|tx| {
        let mut leaf_block = LeafBlock::get(tx, epoch)?;
        // Include the block we last proposed if applicable.
        if let Some(last_proposed) = LastProposed::get(tx, epoch).optional()? &&
            last_proposed.epoch == leaf_block.epoch() &&
            last_proposed.height > leaf_block.height()
        {
            leaf_block = last_proposed.as_leaf_block();
        }
        Ok::<_, HotStuffError>(leaf_block)
    })?;

    if leaf_block.epoch() != msg.epoch || leaf_block.height.is_zero() || leaf_block.height() < msg.from_height {
        info!(
            target: LOG_TARGET,
            "Received catch up request from {from} from block {}/{} but our leaf block is {leaf_block}. Sending an \
             empty page.",
            msg.epoch,
            msg.from_height,
        );
        return Ok(page);
    }

    let start_height = msg.from_height.max(NodeHeight(1));
    let mut used_bytes = 0usize;
    let mut included_transactions = HashSet::new();
    'batches: for result in CatchUpBatches::new(store, msg.epoch, start_height, leaf_block.height()) {
        for block in result? {
            let height = block.height();
            let is_leaf = block.id() == leaf_block.block_id();
            let foreign_proposals = store.with_read_tx(|tx| block.get_foreign_proposals(tx))?;
            let proposal = ProposalMessage {
                block,
                foreign_proposals: foreign_proposals.into_iter().map(|p| p.into_proposal()).collect(),
            };
            let (proposal, size) = measure(outbound_messaging, proposal)?;
            let carries_blocks = !page.blocks.is_empty() || page.leaf.is_some();
            if carries_blocks && (used_bytes + size > page_bytes || page.blocks.len() >= MAX_CATCH_UP_PAGE_BLOCKS) {
                // A page that stops partway through a height resumes at that height, so its remaining forks are
                // sent next, unless the page holds nothing but that height, which would only repeat itself.
                let first_height = page.blocks.first().map(|b| b.block.height()).unwrap_or(height);
                page.next_height = Some(if first_height < height {
                    height
                } else {
                    height + NodeHeight(1)
                });
                break 'batches;
            }
            used_bytes += size;

            for transaction_id in proposal.block.all_transaction_ids() {
                if page.transactions.len() >= MAX_REQUESTED_TRANSACTIONS ||
                    !included_transactions.insert(*transaction_id)
                {
                    continue;
                }
                let Some(record) = store
                    .with_read_tx(|tx| TransactionRecord::get(tx, transaction_id))
                    .optional()?
                else {
                    continue;
                };
                let encoded = EncodedTransaction::encode(record.transaction()).map_err(|e| {
                    HotStuffError::InvariantError(format!("Failed to encode transaction {transaction_id}: {e}"))
                })?;
                let size = encoded.len() + TRANSACTION_FRAMING_BYTES;
                if used_bytes + size > page_bytes {
                    continue;
                }
                used_bytes += size;
                page.transactions.push(encoded);
            }

            // The leaf has no certificate yet, so it travels apart from the certified blocks: the requester
            // handles it as a live proposal and votes on it if it is still current.
            if is_leaf {
                page.leaf = Some(proposal);
            } else {
                page.blocks.push(proposal);
            }
        }
    }

    info!(
        target: LOG_TARGET,
        "🌐 Sending {} block(s), leaf: {}, {} transaction(s) ({used_bytes} bytes) from {}/{} to {from}, next: {:?}",
        page.blocks.len(),
        page.leaf.is_some(),
        page.transactions.len(),
        msg.epoch,
        start_height,
        page.next_height,
    );
    Ok(page)
}

/// `proposal` and the bytes it takes on the wire as a catch-up block.
fn measure<TOutbound: OutboundMessaging>(
    outbound_messaging: &TOutbound,
    proposal: ProposalMessage,
) -> Result<(ProposalMessage, usize), HotStuffError> {
    let message = HotstuffMessage::new_catch_up_sync_response(proposal);
    let size = outbound_messaging.encoded_len(&message);
    let proposal = message
        .into_catch_up_sync_response()
        .ok_or_else(|| HotStuffError::InvariantError("new_catch_up_sync_response built another message".to_string()))?;
    Ok((proposal, size))
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
            let mut blocks = Block::get_all_blocks_between(
                tx,
                self.epoch,
                self.next_height,
                self.leaf_height,
                false,
                CATCH_UP_BATCH_SIZE,
            )?;
            // `Block::get_all_blocks_between` orders by `(epoch, height, block_id)`, so a full batch can stop partway
            // through a height that holds competing forks. Each batch ends on a complete height so that `next_height`
            // advances to `last_height + 1` with every non-dummy block at `last_height` included.
            if blocks.len() == CATCH_UP_BATCH_SIZE &&
                let Some(last) = blocks.last()
            {
                let last_height = last.height();
                let siblings = Block::get_all_blocks_between(
                    tx,
                    self.epoch,
                    last_height,
                    last_height,
                    false,
                    CATCH_UP_BATCH_SIZE,
                )?;
                for sibling in siblings {
                    if blocks.iter().all(|b| b.id() != sibling.id()) {
                        blocks.push(sibling);
                    }
                }
            }
            Ok(blocks)
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
    use crate::traits::{CertificateStore, OutboundMessagingError};

    /// Measures every catch-up block at `BLOCK_BYTES`, and sends nothing.
    struct FixedSizeOutbound;

    const BLOCK_BYTES: usize = 100;

    impl OutboundMessaging for FixedSizeOutbound {
        type Addr = String;

        fn encoded_len(&self, _message: &HotstuffMessage) -> usize {
            BLOCK_BYTES
        }

        async fn send_self<T: Into<HotstuffMessage> + Send>(
            &mut self,
            _message: T,
        ) -> Result<(), OutboundMessagingError> {
            Ok(())
        }

        async fn send<T: Into<HotstuffMessage> + Send>(
            &mut self,
            _to: Self::Addr,
            _message: T,
        ) -> Result<(), OutboundMessagingError> {
            Ok(())
        }

        async fn multicast<T, I>(&mut self, _addresses: I, _message: T) -> Result<(), OutboundMessagingError>
        where
            I: IntoIterator<Item = Self::Addr> + Send,
            T: Into<HotstuffMessage> + Send,
        {
            Ok(())
        }

        async fn broadcast<T>(&mut self, _message: T) -> Result<(), OutboundMessagingError>
        where T: Into<HotstuffMessage> + Send {
            Ok(())
        }
    }

    fn page(
        store: &RocksDbStateStore<String>,
        epoch: Epoch,
        from_height: u64,
        page_bytes: usize,
    ) -> CatchUpPageMessage {
        let request = CatchUpRequestMessage {
            request_id: 7,
            epoch,
            from_height: NodeHeight(from_height),
        };
        read_page(store, &FixedSizeOutbound, &"peer", Epoch::zero(), &request, page_bytes).unwrap()
    }

    fn heights(page: &CatchUpPageMessage) -> Vec<u64> {
        page.blocks.iter().map(|p| p.block.height().as_u64()).collect()
    }

    #[test]
    fn a_page_holds_the_blocks_that_fit_and_says_where_the_next_starts() {
        let (store, _tmp) = store_with_chain(6);

        let first = page(&store, Epoch::zero(), 1, 2 * BLOCK_BYTES);
        assert_eq!(first.request_id, 7);
        assert_eq!(heights(&first), [1, 2]);
        assert!(first.leaf.is_none());
        assert_eq!(first.next_height, Some(NodeHeight(3)));

        let last = page(&store, Epoch::zero(), 5, 2 * BLOCK_BYTES);
        assert_eq!(heights(&last), [5], "the leaf travels apart from the certified blocks");
        assert_eq!(last.leaf.as_ref().map(|leaf| leaf.block.height()), Some(NodeHeight(6)));
        assert_eq!(last.next_height, None);
    }

    #[test]
    fn a_page_carries_one_block_even_when_it_alone_is_over_the_budget() {
        let (store, _tmp) = store_with_chain(3);

        let page = page(&store, Epoch::zero(), 1, BLOCK_BYTES / 2);
        assert_eq!(heights(&page), [1]);
        assert_eq!(page.next_height, Some(NodeHeight(2)));
    }

    #[test]
    fn a_request_this_node_cannot_serve_gets_an_empty_page() {
        let (store, _tmp) = store_with_chain(3);

        for (epoch, from_height) in [(Epoch(1), 1), (Epoch::zero(), 4)] {
            let page = page(&store, epoch, from_height, 10 * BLOCK_BYTES);
            assert!(page.blocks.is_empty() && page.leaf.is_none(), "{epoch}/{from_height}");
            assert_eq!(page.next_height, None);
            assert_eq!(page.request_id, 7);
        }
    }

    const NETWORK: Network = Network::LocalNet;
    const NUM_PRESHARDS: NumPreshards = NumPreshards::P256;

    /// A block extending `parent`. It carries a signature, since a catch-up response skips unsigned (dummy) blocks;
    /// the store does not check it.
    fn child(parent: &Block) -> Block {
        child_with_fee(parent, 0)
    }

    fn child_with_fee(parent: &Block, total_leader_fee: u64) -> Block {
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
            total_leader_fee,
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
                block.as_leaf().set(tx)?;
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

    #[test]
    fn sibling_blocks_at_a_batch_boundary_height_are_not_skipped() {
        let boundary = CATCH_UP_BATCH_SIZE as u64;
        let leaf_height = boundary + 1;
        let tmp = tempfile::tempdir().unwrap();
        let store = RocksDbStateStore::<String>::open(tmp.path(), DatabaseOptions::default()).unwrap();
        let mut block = Block::zero_block(NETWORK, NUM_PRESHARDS);
        let (fork_a, fork_b, leaf) = store
            .with_write_tx(|tx| {
                block.justify().save(tx)?;
                block.insert(tx)?;
                for _ in 1..boundary {
                    block = child(&block);
                    block.justify().save(tx)?;
                    block.insert(tx)?;
                }
                let fork_a = child_with_fee(&block, 0);
                let fork_b = child_with_fee(&block, 1);
                for fork in [&fork_a, &fork_b] {
                    fork.justify().save(tx)?;
                    fork.insert(tx)?;
                }
                // Extend whichever sibling sorts second by BlockId so it sits past the 100-block limit.
                let canonical_parent = if fork_a.id() > fork_b.id() { &fork_a } else { &fork_b };
                let leaf = child(canonical_parent);
                leaf.justify().save(tx)?;
                leaf.insert(tx)?;
                Ok::<_, StorageError>((fork_a, fork_b, leaf))
            })
            .unwrap();

        let ids: Vec<_> = CatchUpBatches::new(&store, Epoch::zero(), NodeHeight(1), NodeHeight(leaf_height))
            .flat_map(|batch| batch.unwrap().into_iter().map(|b| *b.id()))
            .collect();

        assert!(ids.contains(fork_a.id()), "missing fork_a at boundary height");
        assert!(ids.contains(fork_b.id()), "missing fork_b at boundary height");
        assert_eq!(ids.last(), Some(leaf.id()));
    }
}
