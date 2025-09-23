//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use futures::StreamExt;
use log::*;
use tari_consensus_types::BlockId;
use tari_epoch_manager::{service::EpochManagerHandle, EpochManagerReader};
use tari_ootle_common_types::{committee::Committee, Epoch, PeerAddress, ShardGroup};
use tari_ootle_p2p::{proto, proto::rpc::SyncBlocksRequest};
use tari_ootle_storage::consensus_models::{Block, SubstateUpdateProof};
use tari_validator_node_rpc::client::{TariValidatorNodeRpcClientFactory, ValidatorNodeClientFactory};

use crate::{
    block_data::BlockData,
    storage_sqlite::{
        models::NewScannedBlockId,
        IndexerStore,
        IndexerStoreReadTransaction,
        IndexerStoreWriteTransaction,
        SqliteIndexerStore,
    },
};

const LOG_TARGET: &str = "tari::indexer::block_scanner";

#[derive(Clone)]
pub struct BlockScanner {
    epoch_manager: EpochManagerHandle<PeerAddress>,
    client_factory: TariValidatorNodeRpcClientFactory,
    substate_store: SqliteIndexerStore,
}

impl BlockScanner {
    pub fn new(
        epoch_manager: EpochManagerHandle<PeerAddress>,
        client_factory: TariValidatorNodeRpcClientFactory,
        substate_store: SqliteIndexerStore,
    ) -> Self {
        Self {
            epoch_manager,
            client_factory,
            substate_store,
        }
    }

    pub async fn scan(&self) -> Result<usize, anyhow::Error> {
        let mut block_count = 0;

        let newest_epoch = self.epoch_manager.current_epoch().await?;
        let oldest_scanned_epoch = self.get_oldest_scanned_epoch().await?;

        match oldest_scanned_epoch {
            Some(oldest_epoch) => {
                // we could span concurrent epoch scans
                // but we want to avoid gaps of the latest scanned value if any of the intermediate epoch scans fail
                for epoch in oldest_epoch.as_u64()..=newest_epoch.as_u64() {
                    let epoch = Epoch(epoch);
                    block_count += self.scan_blocks_in_epoch(epoch).await?;

                    // at this point we can assume the previous epochs have been fully scanned
                    self.delete_scanned_epochs_older_than(epoch).await?;
                }
            },
            None => {
                // by default we start scanning since the current epoch
                // TODO: maybe a new parameter in the indexer to specify a custom starting epoch, or we should scan from
                // the genesis epoch
                block_count += self.scan_blocks_in_epoch(newest_epoch).await?;
            },
        }

        info!(
            target: LOG_TARGET,
            "Scanned {} events",
            block_count
        );

        Ok(block_count)
    }

    async fn scan_blocks_in_epoch(&self, epoch: Epoch) -> Result<usize, anyhow::Error> {
        // TODO(perf): This call can become expensive. Lazily load a committee member from all shard groups
        let committees = self.epoch_manager.get_committees(epoch).await?;
        let mut count = 0;

        for (shard_group, committee) in committees {
            info!(
                target: LOG_TARGET,
                "Scanning committee epoch={}, sg={}",
                epoch,
                shard_group
            );
            let new_blocks = self
                .get_new_blocks_from_committee(shard_group, &committee, epoch)
                .await?;
            info!(
                target: LOG_TARGET,
                "Scanned {} blocks in epoch={}",
                new_blocks.len(),
                epoch,
            );

            count += new_blocks.len();
            for block_data in new_blocks {
                // TODO: store blocks
                // TODO: remove substates (I think). These can be requested lazily and cached (LRU) as needed to allow
                // TODO: an committed transaction should queue a shard state sync in the affected shards
                // an upper bound on substates stored in the indexer.
                info!(
                    target: LOG_TARGET,
                    "Storing {} substate update(s) for block {} (epoch={}, height={})",
                    block_data.diff.len(),
                    block_data.block.id(),
                    block_data.block.epoch(),
                    block_data.block.height()
                );
                self.store_substates_in_db(&block_data.diff)?;
            }
        }

        Ok(count)
    }

    async fn delete_scanned_epochs_older_than(&self, epoch: Epoch) -> Result<(), anyhow::Error> {
        self.substate_store
            .with_write_tx(|tx| tx.delete_scanned_epochs_older_than(epoch))
            .map_err(|e| e.into())
    }

    fn store_substates_in_db(&self, updates: &[SubstateUpdateProof]) -> Result<(), anyhow::Error> {
        let mut tx = self.substate_store.create_write_tx()?;
        // store/update up substates if any
        for update in updates {
            match update {
                SubstateUpdateProof::Create(create) => {
                    if create.substate.value.value().is_none() {
                        warn!(
                            target: LOG_TARGET,
                            "⚠️ Received UP substate {} without value. This indicates that the substate has been pruned. Some event data is not available.", create.substate.as_versioned_substate_id_ref(),
                        );
                    }
                    debug!(
                        target: LOG_TARGET,
                        "Saving substate: {:?}",
                        create.substate
                    );
                    tx.upsert_substate(&create.substate)?;
                },
                SubstateUpdateProof::Destroy(_) => {},
            }
        }
        tx.commit()?;
        Ok(())
    }

    async fn get_oldest_scanned_epoch(&self) -> Result<Option<Epoch>, anyhow::Error> {
        self.substate_store
            .with_read_tx(|tx| tx.get_oldest_scanned_epoch())
            .map_err(|e| e.into())
    }

    async fn get_new_blocks_from_committee(
        &self,
        shard_group: ShardGroup,
        committee: &Committee<PeerAddress>,
        epoch: Epoch,
    ) -> Result<Vec<BlockData>, anyhow::Error> {
        // We start scanning from the last scanned block for this committee
        let start_block_id = self
            .substate_store
            .with_read_tx(|tx| tx.get_last_scanned_block_id(epoch, shard_group))?;

        info!(
            target: LOG_TARGET,
            "Scanning new blocks from (start_id={}, epoch={}, shard={})",
            start_block_id.map(|id| id.to_string()).unwrap_or_else(|| "None".to_string()),
            epoch,
            shard_group
        );

        for member in committee.shuffled() {
            debug!(
                target: LOG_TARGET,
                "Trying to get blocks from VN {} (epoch={}, shard_group={})",
                member,
                epoch,
                shard_group
            );
            let resp = self
                .get_block_data_from_vn(&member.address, start_block_id, epoch)
                .await;

            match resp {
                Ok(block_data) => {
                    // TODO: try more than 1 VN per committee
                    info!(
                        target: LOG_TARGET,
                        "Got {} blocks from VN {} (epoch={}, shard_group={})",
                        block_data.len(),
                        member,
                        epoch,
                        shard_group,
                    );

                    // get the most recent block among all scanned blocks in the epoch
                    let last_block = block_data
                        .iter()
                        .max_by_key(|data| (data.block.epoch(), data.block.height()))
                        .map(|data| &data.block);

                    if let Some(block) = last_block {
                        // Store the latest scanned block id in the database for future scans
                        self.save_scanned_block_id(epoch, shard_group, *block.id())?;
                    }
                    return Ok(block_data);
                },
                Err(e) => {
                    // We do nothing on a single VN failure, we only log it
                    warn!(
                        target: LOG_TARGET,
                        "Could not get blocks from VN {} (epoch={}, shard_group={}): {}",
                        member,
                        epoch,
                        shard_group,
                        e
                    );
                },
            };
        }

        // We don't raise an error if none of the VNs have blocks, the scanning will retry eventually
        warn!(
            target: LOG_TARGET,
            "Could not get blocks from any of the VNs of the committee (epoch={}, shard_group={})",
            epoch,
            shard_group
        );
        Ok(vec![])
    }

    fn save_scanned_block_id(
        &self,
        epoch: Epoch,
        shard_group: ShardGroup,
        last_block_id: BlockId,
    ) -> Result<(), anyhow::Error> {
        let row = NewScannedBlockId {
            epoch: epoch.0 as i64,
            shard_group: shard_group.encode_as_u32() as i32,
            last_block_id: last_block_id.as_bytes().to_vec(),
        };
        self.substate_store.with_write_tx(|tx| tx.save_scanned_block_id(row))?;
        Ok(())
    }

    // async fn get_all_vns(&self) -> Result<Vec<PeerAddress>, anyhow::Error> {
    //     // get all the committees
    //     let epoch = self.epoch_manager.current_epoch().await?;
    //     Ok(self
    //         .epoch_manager
    //         .get_all_validator_nodes(epoch)
    //         .await
    //         .map(|v| v.iter().map(|m| m.address).collect())?)
    // }

    async fn get_block_data_from_vn(
        &self,
        vn_addr: &PeerAddress,
        start_block_id: Option<BlockId>,
        up_to_epoch: Epoch,
    ) -> Result<Vec<BlockData>, anyhow::Error> {
        let mut blocks = vec![];

        let rpc_client = self.client_factory.create_client(vn_addr);
        let mut client = rpc_client.client_connection().await?;

        let mut stream = client
            .sync_blocks(SyncBlocksRequest {
                start_block_id: start_block_id.map(|id| id.as_bytes().to_vec()).unwrap_or_default(),
                epoch: Some(up_to_epoch.into()),
                // TODO: QC should be validated
                stream_qcs: false,
                stream_substates: proto::rpc::StreamSubstateSelection::AllSubstates.into(),
                stream_transactions: false,
            })
            .await?;
        while let Some(resp) = stream.next().await {
            let msg = resp?;

            let new_block = msg
                .into_block()
                .ok_or_else(|| anyhow::anyhow!("Expected peer to return a newblock"))?;
            let block = Block::try_from(new_block)?;

            let Some(resp) = stream.next().await else {
                anyhow::bail!("Peer closed session before sending substate update count message")
            };
            let msg = resp?;
            let num_substates =
                msg.substate_count()
                    .ok_or_else(|| anyhow::anyhow!("Expected peer to return substate count"))? as usize;
            // TODO: what should this limit be?
            if num_substates > 10_000 {
                return Err(anyhow::anyhow!(
                    "Exceeded maximum number of substates (10,000). Got {}",
                    num_substates
                ));
            }

            let mut diff = Vec::with_capacity(num_substates);
            for _ in 0..num_substates {
                let Some(resp) = stream.next().await else {
                    anyhow::bail!("Peer closed session before sending substate updates message")
                };
                let msg = resp?;
                let update = msg
                    .into_substate_update()
                    .ok_or_else(|| anyhow::anyhow!("Expected a substate"))?;
                let update = SubstateUpdateProof::try_from(update)?;
                diff.push(update);
            }

            blocks.push(BlockData { block, diff });
        }

        Ok(blocks)
    }
}
