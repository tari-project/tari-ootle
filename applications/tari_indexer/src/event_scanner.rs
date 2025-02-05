//  Copyright 2024 The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::str::FromStr;

use futures::StreamExt;
use log::*;
use tari_crypto::tari_utilities::message_format::MessageFormat;
use tari_dan_common_types::{committee::Committee, Epoch, PeerAddress, ShardGroup};
use tari_dan_p2p::{proto, proto::rpc::SyncBlocksRequest};
use tari_dan_storage::consensus_models::{Block, BlockId, SubstateUpdate};
use tari_engine_types::{
    events::Event,
    substate::{SubstateId, SubstateValue},
};
use tari_epoch_manager::{base_layer::EpochManagerHandle, EpochManagerReader};
use tari_template_lib::models::{EntityId, TemplateAddress};
use tari_validator_node_rpc::client::{TariValidatorNodeRpcClientFactory, ValidatorNodeClientFactory};

use crate::{
    block_data::BlockData,
    config::EventFilterConfig,
    substate_storage_sqlite::{
        models::{
            events::{NewEvent, NewScannedBlockId},
            substate::NewSubstate,
        },
        sqlite_substate_store_factory::{
            SqliteSubstateStore,
            SubstateStore,
            SubstateStoreReadTransaction,
            SubstateStoreWriteTransaction,
        },
    },
};

const LOG_TARGET: &str = "tari::indexer::event_scanner";

#[derive(Default, Debug, Clone)]
pub struct EventFilter {
    pub topic: Option<String>,
    pub entity_id: Option<EntityId>,
    pub substate_id: Option<SubstateId>,
    pub template_address: Option<TemplateAddress>,
}

impl TryFrom<EventFilterConfig> for EventFilter {
    type Error = anyhow::Error;

    fn try_from(cfg: EventFilterConfig) -> Result<Self, Self::Error> {
        let entity_id = cfg.entity_id.map(|str| EntityId::from_hex(&str)).transpose()?;
        let substate_id = cfg.substate_id.map(|str| SubstateId::from_str(&str)).transpose()?;
        let template_address = cfg
            .template_address
            .map(|str| TemplateAddress::from_str(&str))
            .transpose()?;

        Ok(Self {
            topic: cfg.topic,
            entity_id,
            substate_id,
            template_address,
        })
    }
}

pub struct EventScanner {
    epoch_manager: EpochManagerHandle<PeerAddress>,
    client_factory: TariValidatorNodeRpcClientFactory,
    substate_store: SqliteSubstateStore,
    event_filters: Vec<EventFilter>,
}

impl EventScanner {
    pub fn new(
        epoch_manager: EpochManagerHandle<PeerAddress>,
        client_factory: TariValidatorNodeRpcClientFactory,
        substate_store: SqliteSubstateStore,
        event_filters: Vec<EventFilter>,
    ) -> Self {
        Self {
            epoch_manager,
            client_factory,
            substate_store,
            event_filters,
        }
    }

    pub async fn scan_events(&self) -> Result<usize, anyhow::Error> {
        info!(
            target: LOG_TARGET,
            "scan_events",
        );

        let mut event_count = 0;

        let newest_epoch = self.epoch_manager.current_epoch().await?;
        let oldest_scanned_epoch = self.get_oldest_scanned_epoch().await?;

        match oldest_scanned_epoch {
            Some(oldest_epoch) => {
                // we could span multiple cuncurrent epoch scans
                // but we want to avoid gaps of the latest scanned value if any of the intermediate epoch scans fail
                for epoch_idx in oldest_epoch.0..=newest_epoch.0 {
                    let epoch = Epoch(epoch_idx);
                    event_count += self.scan_events_of_epoch(epoch).await?;

                    // at this point we can assume the previous epochs have been fully scanned
                    self.delete_scanned_epochs_older_than(epoch).await?;
                }
            },
            None => {
                // by default we start scanning since the current epoch
                // TODO: it would be nice a new parameter in the indexer to specify a custom starting epoch
                event_count += self.scan_events_of_epoch(newest_epoch).await?;
            },
        }

        info!(
            target: LOG_TARGET,
            "Scanned {} events",
            event_count
        );

        Ok(event_count)
    }

    async fn scan_events_of_epoch(&self, epoch: Epoch) -> Result<usize, anyhow::Error> {
        let committees = self.epoch_manager.get_committees(epoch).await?;

        let mut event_count = 0;

        for (shard_group, mut committee) in committees {
            info!(
                target: LOG_TARGET,
                "Scanning committee epoch={}, sg={}",
                epoch,
                shard_group
            );
            let new_blocks = self
                .get_new_blocks_from_committee(shard_group, &mut committee, epoch)
                .await?;
            info!(
                target: LOG_TARGET,
                "Scanned {} blocks in epoch={}",
                new_blocks.len(),
                epoch,
            );

            for block_data in new_blocks {
                // fetch all the events in the transaction
                let events = block_data
                    .diff
                    .iter()
                    .filter_map(|r| r.as_create())
                    .filter_map(|create| create.substate.substate_value.as_transaction_receipt())
                    .flat_map(|receipt| receipt.events.as_slice());
                event_count += events.clone().count();

                // only keep the events specified by the indexer filter
                let filtered_events: Vec<_> = events.filter(|ev| self.should_persist_event(ev)).collect();
                info!(
                    target: LOG_TARGET,
                    "Filtered events in epoch {}: {}",
                    epoch,
                    filtered_events.len()
                );
                self.store_events_in_db(&filtered_events, block_data.block.timestamp())?;
                self.store_substates_in_db(&block_data.diff, block_data.block.timestamp())?;
            }
        }

        Ok(event_count)
    }

    async fn delete_scanned_epochs_older_than(&self, epoch: Epoch) -> Result<(), anyhow::Error> {
        self.substate_store
            .with_write_tx(|tx| tx.delete_scanned_epochs_older_than(epoch))
            .map_err(|e| e.into())
    }

    fn should_persist_event(&self, event: &Event) -> bool {
        for filter in &self.event_filters {
            if Self::event_matches_filter(filter, event) {
                return true;
            }
        }

        false
    }

    fn event_matches_filter(filter: &EventFilter, event: &Event) -> bool {
        let matches_topic = filter.topic.as_ref().map_or(true, |t| *t == event.topic());
        let matches_template = filter
            .template_address
            .as_ref()
            .map_or(true, |t| *t == event.template_address());

        let matches_substate_id = match filter.substate_id {
            Some(ref substate_id) => event.substate_id().map(|s| s == substate_id).unwrap_or(false),
            None => true,
        };

        let matches_entity_id = match &filter.entity_id {
            Some(entity_id) => event
                .substate_id()
                .map(|s| Self::entity_id_matches(s, entity_id))
                .unwrap_or(false),
            None => true,
        };

        if matches_topic && matches_template && matches_substate_id && matches_entity_id {
            return true;
        }

        false
    }

    fn entity_id_matches(substate_id: &SubstateId, entity_id: &EntityId) -> bool {
        substate_id.to_object_key().as_entity_id() == *entity_id
    }

    fn store_events_in_db(&self, events: &[&Event], timestamp: u64) -> Result<(), anyhow::Error> {
        let mut tx = self.substate_store.create_write_tx()?;

        for event in events {
            let event_row = NewEvent {
                template_address: event.template_address().to_string(),
                tx_hash: event.tx_hash().to_string(),
                topic: event.topic(),
                payload: event.payload().to_json().expect("Failed to convert to JSON"),
                substate_id: event.substate_id().map(|s| s.to_string()),
                // TODO: include substate version in event?
                version: 0_i32,
                timestamp: timestamp as i64,
            };

            // TODO: properly avoid or handle duplicated events
            //       For now we will just check if a similar event exists in the db
            let event_already_exists = tx.event_exists(&event_row)?;
            if event_already_exists {
                // the event was already stored previously
                // TODO: Making this debug because it happens a lot and tends to spam the swarm output
                debug!(
                    target: LOG_TARGET,
                    "Duplicate {}",
                    event
                );
                continue;
            }

            debug!(
                target: LOG_TARGET,
                "Saving event: {:?}",
                event_row
            );
            tx.save_event(event_row)?;
        }

        tx.commit()?;

        Ok(())
    }

    fn store_substates_in_db(&self, updates: &[SubstateUpdate], timestamp: u64) -> Result<(), anyhow::Error> {
        let mut tx = self.substate_store.create_write_tx()?;
        // store/update up substates if any
        for create in updates.iter().filter_map(|up| up.as_create()) {
            let template_address = Self::extract_template_address_from_substate(&create.substate.substate_value);
            let module_name = Self::extract_module_name_from_substate(&create.substate.substate_value);
            let substate_row = NewSubstate {
                address: create.substate.substate_id.to_string(),
                version: i64::from(create.substate.version),
                data: Self::encode_substate(&create.substate.substate_value)?,
                tx_hash: create.substate.created_by_transaction.to_string(),
                template_address: template_address.map(|s| s.to_string()),
                module_name,
                timestamp: timestamp as i64,
            };
            debug!(
                target: LOG_TARGET,
                "Saving substate: {:?}",
                substate_row
            );
            tx.set_substate(substate_row)?;
        }
        tx.commit()?;
        Ok(())
    }

    fn extract_template_address_from_substate(substate: &SubstateValue) -> Option<TemplateAddress> {
        match substate {
            SubstateValue::Component(c) => Some(c.template_address),
            _ => None,
        }
    }

    fn extract_module_name_from_substate(substate: &SubstateValue) -> Option<String> {
        match substate {
            SubstateValue::Component(c) => Some(c.module_name.to_owned()),
            _ => None,
        }
    }

    fn encode_substate(substate: &SubstateValue) -> Result<String, anyhow::Error> {
        let pretty_json = serde_json::to_string_pretty(&substate)?;
        Ok(pretty_json)
    }

    async fn get_oldest_scanned_epoch(&self) -> Result<Option<Epoch>, anyhow::Error> {
        self.substate_store
            .with_read_tx(|tx| tx.get_oldest_scanned_epoch())
            .map_err(|e| e.into())
    }

    #[allow(unused_assignments)]
    async fn get_new_blocks_from_committee(
        &self,
        shard_group: ShardGroup,
        committee: &mut Committee<PeerAddress>,
        epoch: Epoch,
    ) -> Result<Vec<BlockData>, anyhow::Error> {
        // We start scanning from the last scanned block for this committee
        let start_block_id = self
            .substate_store
            .with_read_tx(|tx| tx.get_last_scanned_block_id(epoch, shard_group))?;

        committee.shuffle();
        let mut last_block_id = start_block_id;

        info!(
            target: LOG_TARGET,
            "Scanning new blocks from (start_id={}, epoch={}, shard={})",
            last_block_id.map(|id| id.to_string()).unwrap_or_else(|| "None".to_string()),
            epoch,
            shard_group
        );

        for member in committee.members() {
            debug!(
                target: LOG_TARGET,
                "Trying to get blocks from VN {} (epoch={}, shard_group={})",
                member,
                epoch,
                shard_group
            );
            let resp = self.get_block_data_from_vn(member, last_block_id, epoch).await;

            match resp {
                Ok(block_data) => {
                    // TODO: try more than 1 VN per commitee
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
                        last_block_id = Some(*block.id());
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

        let mut rpc_client = self.client_factory.create_client(vn_addr);
        let mut client = rpc_client.client_connection().await?;

        let mut stream = client
            .sync_blocks(SyncBlocksRequest {
                start_block_id: start_block_id.map(|id| id.as_bytes().to_vec()).unwrap_or_default(),
                epoch: Some(up_to_epoch.into()),
                // TODO: QC should be validated
                stream_qcs: false,
                stream_substates: proto::rpc::StreamSubstateSelection::All.into(),
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
                let update = SubstateUpdate::try_from(update)?;
                diff.push(update);
            }

            blocks.push(BlockData { block, diff });
        }

        Ok(blocks)
    }
}
