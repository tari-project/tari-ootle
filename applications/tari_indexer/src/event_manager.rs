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

use std::{collections::BTreeMap, str::FromStr, sync::Arc};

use log::*;
use tari_crypto::tari_utilities::message_format::MessageFormat;
use tari_dan_app_utilities::substate_file_cache::SubstateFileCache;
use tari_dan_common_types::PeerAddress;
use tari_engine_types::{events::Event, substate::SubstateId};
use tari_epoch_manager::service::EpochManagerHandle;
use tari_indexer_lib::substate_scanner::SubstateScanner;
use tari_template_lib::{
    models::Metadata,
    types::{Hash, TemplateAddress},
};
use tari_transaction::TransactionId;
use tari_validator_node_rpc::client::TariValidatorNodeRpcClientFactory;

use crate::substate_storage_sqlite::{
    models::events::NewEvent,
    sqlite_substate_store_factory::{
        SqliteSubstateStore,
        SubstateStore,
        SubstateStoreReadTransaction,
        SubstateStoreWriteTransaction,
    },
};

const LOG_TARGET: &str = "tari::indexer::event_manager";

pub struct EventManager {
    substate_store: SqliteSubstateStore,
    _substate_scanner:
        Arc<SubstateScanner<EpochManagerHandle<PeerAddress>, TariValidatorNodeRpcClientFactory, SubstateFileCache>>,
}

impl EventManager {
    pub fn new(
        substate_store: SqliteSubstateStore,
        substate_scanner: Arc<
            SubstateScanner<EpochManagerHandle<PeerAddress>, TariValidatorNodeRpcClientFactory, SubstateFileCache>,
        >,
    ) -> Self {
        Self {
            substate_store,
            _substate_scanner: substate_scanner,
        }
    }

    pub fn save_event_to_db(
        &self,
        substate_id: &SubstateId,
        template_address: TemplateAddress,
        tx_hash: TransactionId,
        topic: String,
        payload: &Metadata,
        version: u64,
        timestamp: u64,
    ) -> Result<(), anyhow::Error> {
        self.substate_store.with_write_tx(|tx| {
            let new_event = NewEvent {
                substate_id: Some(substate_id.to_string()),
                template_address: template_address.to_string(),
                tx_hash: tx_hash.to_string(),
                topic,
                payload: payload.to_json().expect("Failed to convert to JSON"),
                version: version as i32,
                timestamp: timestamp as i64,
            };
            tx.save_event(new_event)
        })?;
        Ok(())
    }

    pub async fn get_events_from_db(
        &self,
        topic: Option<String>,
        substate_id: Option<SubstateId>,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Event>, anyhow::Error> {
        let rows = self
            .substate_store
            .with_read_tx(|tx| tx.get_events(substate_id, topic, offset, limit))?;

        debug!(target: LOG_TARGET, "Found {} events", rows.len());

        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            let substate_id = row.substate_id.map(|str| SubstateId::from_str(&str)).transpose()?;
            let template_address = Hash::from_hex(&row.template_address)?;
            let tx_hash = Hash::from_hex(&row.tx_hash)?;
            let topic = row.topic;
            let payload = Metadata::from(serde_json::from_str::<BTreeMap<String, String>>(row.payload.as_str())?);
            events.push(Event::new(substate_id, template_address, tx_hash, topic, payload));
        }

        Ok(events)
    }
}
