//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use log::debug;
use tari_consensus_types::{HighPc, HighestSeenBlock, LastExecuted, LockedBlock, ProposalCertificate};
use tari_ootle_common_types::optional::Optional;
use tari_ootle_storage::{
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
    consensus_models::{Block, BookkeepingModel},
};

use crate::traits::CertificateStore;

const LOG_TARGET: &str = "tari::ootle::storage::block_store";

pub trait BlockStore {
    fn update_nodes<TTx, TFnOnLock, TFnOnCommit, E>(
        &self,
        tx: &mut TTx,
        on_lock_block: TFnOnLock,
        on_commit: TFnOnCommit,
    ) -> Result<HighPc, E>
    where
        TTx: StateStoreWriteTransaction + Deref,
        TTx::Target: StateStoreReadTransaction,
        TFnOnLock: FnMut(&mut TTx, &LockedBlock, &Block, &ProposalCertificate) -> Result<(), E>,
        TFnOnCommit: FnMut(&mut TTx, Block) -> Result<(), E>,
        E: From<StorageError>;
}

impl BlockStore for Block {
    fn update_nodes<TTx, TFnOnLock, TFnOnCommit, E>(
        &self,
        tx: &mut TTx,
        mut on_lock_block: TFnOnLock,
        mut on_commit: TFnOnCommit,
    ) -> Result<HighPc, E>
    where
        TTx: StateStoreWriteTransaction + Deref,
        TTx::Target: StateStoreReadTransaction,
        TFnOnLock: FnMut(&mut TTx, &LockedBlock, &Block, &ProposalCertificate) -> Result<(), E>,
        TFnOnCommit: FnMut(&mut TTx, Block) -> Result<(), E>,
        E: From<StorageError>,
    {
        if HighestSeenBlock::get(&**tx, self.epoch())
            .optional()?
            .is_none_or(|h| h.height < self.height())
        {
            self.as_highest_seen().set(tx)?;
        }
        let high_qc = self.justify().update_highest(tx)?;

        // b'' <- b*.justify.node i.e. the (possibly new) justified block
        let justified_node = Block::get(&**tx, &self.justify().calculate_block_id())?;

        // b' <- b''.justify.node
        let new_locked = Block::get(&**tx, &justified_node.justify().calculate_block_id())?;
        if new_locked.is_genesis() {
            return Ok(high_qc);
        }

        let current_locked = LockedBlock::get(&**tx, self.epoch())?;
        if new_locked.height() > current_locked.height {
            on_locked_block_recurse(
                tx,
                &current_locked,
                &new_locked,
                justified_node.justify(),
                &mut on_lock_block,
            )?;
            new_locked.as_locked().set(tx)?;
            // A parked block at or below the locked block can never be voted on
            let num_removed = tx.parked_blocks_remove_up_to(new_locked.epoch(), new_locked.height())?;
            if num_removed > 0 {
                debug!(
                    target: LOG_TARGET,
                    "🗑️ Removed {num_removed} parked block(s) at or below locked block {}",
                    new_locked,
                );
            }
        }

        // b <- b'.justify.node
        let commit_node = new_locked.justify().calculate_block_id();
        // b''.parent() == b'.id() && b'.parent() == b.id()
        if justified_node.parent() == new_locked.id() && *new_locked.parent() == commit_node {
            debug!(
                target: LOG_TARGET,
                "✅ Block {} {} forms a 3-chain b'' = {}, b' = {}, b = {}",
                self.height(),
                self.id(),
                justified_node.id(),
                new_locked.id(),
                commit_node,
            );

            // Commit prepare_node (b)
            if commit_node.is_zero() {
                return Ok(high_qc);
            }
            let prepare_node = Block::get(&**tx, &commit_node)?;
            let last_executed = LastExecuted::get(&**tx, self.epoch())?;
            if last_executed.height < prepare_node.height() {
                let last_exec = prepare_node.as_last_executed();
                on_commit_block_recurse(tx, &last_executed, prepare_node, &mut on_commit)?;
                last_exec.set(tx)?;
            }
        } else {
            debug!(
                target: LOG_TARGET,
                "Block {} {} DOES NOT form a 3-chain b'' = {}, b' = {}, b = {}, b* = {}",
                self.height(),
                self.id(),
                justified_node.id(),
                new_locked.id(),
                commit_node,
                self.id()
            );
        }

        Ok(high_qc)
    }
}

fn on_locked_block_recurse<TTx, F, E>(
    tx: &mut TTx,
    locked: &LockedBlock,
    block: &Block,
    justify_qc: &ProposalCertificate,
    callback: &mut F,
) -> Result<(), E>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
    E: From<StorageError>,
    F: FnMut(&mut TTx, &LockedBlock, &Block, &ProposalCertificate) -> Result<(), E>,
{
    if locked.height < block.height() {
        let parent = block.get_parent(&**tx)?;
        on_locked_block_recurse(tx, locked, &parent, block.justify(), callback)?;
        callback(tx, locked, block, justify_qc)?;
    }
    Ok(())
}

fn on_commit_block_recurse<TTx, F, E>(
    tx: &mut TTx,
    last_executed: &LastExecuted,
    block: Block,
    callback: &mut F,
) -> Result<(), E>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
    E: From<StorageError>,
    F: FnMut(&mut TTx, Block) -> Result<(), E>,
{
    if last_executed.height < block.height() {
        let parent = block.get_parent(&**tx)?;
        // Recurse to "catch up" any parent blocks we may not have executed
        on_commit_block_recurse(tx, last_executed, parent, callback)?;
        callback(tx, block)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tari_consensus_types::{LastExecuted, LockedBlock, ProposalCertificate, ShardGroupAccumulatedData};
    use tari_crypto::tari_utilities::epoch_time::EpochTime;
    use tari_ootle_common_types::{
        Epoch,
        ExtraData,
        NodeHeight,
        NumPreshards,
        ProtocolVersion,
        ShardGroup,
    };
    use tari_ootle_storage::{
        StateStore,
        StorageError,
        consensus_models::{Block, BlockCommands, BlockHeader, BookkeepingModel},
    };
    use tari_ootle_transaction::Network;
    use tari_sidechain::{FixedHash, QuorumDecision};
    use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};
    use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

    use super::BlockStore;

    fn make_block(parent: &Block, justify_block: &Block, height: NodeHeight) -> Block {
        let shard_group = ShardGroup::all_shards(NumPreshards::P256);
        let commands = BlockCommands::empty();
        let justify = ProposalCertificate::new(
            *justify_block.id().hash(),
            *justify_block.id(),
            justify_block.height(),
            justify_block.epoch(),
            shard_group,
            ProposalCertificate::genesis_signatures(),
            QuorumDecision::Accept,
        );
        let header = BlockHeader::create_unsigned(
            Network::LocalNet,
            ProtocolVersion::V0,
            *parent.id(),
            justify.calculate_id(),
            None,
            height,
            Epoch::zero(),
            shard_group,
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
        Block::new(header, justify, commands, None)
    }

    #[test]
    fn proposal_justified_by_locked_block_does_not_rewind_last_executed() {
        let tmp = tempfile::tempdir().unwrap();
        let store: RocksDbStateStore<String> =
            RocksDbStateStore::open(tmp.path(), DatabaseOptions::default()).unwrap();

        let zero = Block::zero_block(Network::LocalNet, NumPreshards::P256);
        let b1 = make_block(&zero, &zero, NodeHeight(1));
        let b2 = make_block(&b1, &b1, NodeHeight(2));
        let b3 = make_block(&b2, &b2, NodeHeight(3));
        let b4 = make_block(&b3, &b3, NodeHeight(4));
        let b5 = make_block(&b4, &b4, NodeHeight(5));
        // A later proposal justified by the locked block b3 (passes is_safe because justify_block == locked_block).
        let b6_fork = make_block(&b3, &b3, NodeHeight(6));

        let mut committed = Vec::new();
        store
            .with_write_tx(|tx| {
                zero.justify().save(tx)?;
                zero.insert(tx)?;
                zero.as_locked().set(tx)?;
                zero.as_last_executed().set(tx)?;

                for block in [&b1, &b2, &b3, &b4, &b5] {
                    block.justify().save(tx)?;
                    block.insert(tx)?;
                    block.update_nodes(
                        tx,
                        |_, _, _, _| Ok::<_, StorageError>(()),
                        |_, b| {
                            committed.push(*b.id());
                            Ok(())
                        },
                    )?;
                }

                assert_eq!(*LockedBlock::get(&**tx, Epoch::zero())?.block_id(), *b3.id());
                assert_eq!(LastExecuted::get(&**tx, Epoch::zero())?.height, NodeHeight(2));
                assert_eq!(committed, vec![*b1.id(), *b2.id()]);

                committed.clear();
                b6_fork.justify().save(tx)?;
                b6_fork.insert(tx)?;
                b6_fork.update_nodes(
                    tx,
                    |_, _, _, _| Ok::<_, StorageError>(()),
                    |_, b| {
                        committed.push(*b.id());
                        Ok(())
                    },
                )?;

                let last_exec = LastExecuted::get(&**tx, Epoch::zero())?;
                assert_eq!(last_exec.height, NodeHeight(2));
                assert_eq!(last_exec.block_id, *b2.id());
                assert!(committed.is_empty());

                Ok::<_, StorageError>(())
            })
            .unwrap();
    }
}
