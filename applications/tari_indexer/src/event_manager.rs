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

use log::*;
use tari_engine_types::{events::Event, substate::SubstateId};
use tari_ootle_transaction::TransactionId;
use tari_template_lib_types::ResourceAddress;

use crate::{
    storage_sqlite::SqliteIndexerStore,
    store::{IndexerStoreReadTransaction, IndexerStoreReader},
};

const LOG_TARGET: &str = "tari::indexer::event_manager";

#[derive(Debug, Clone)]
pub struct EventManager {
    substate_store: SqliteIndexerStore,
}

impl EventManager {
    pub fn new(substate_store: SqliteIndexerStore) -> Self {
        Self { substate_store }
    }

    pub async fn get_events_from_db(
        &self,
        topic: Option<&str>,
        substate_id: Option<&SubstateId>,
        resource_address: Option<&ResourceAddress>,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<(TransactionId, Event)>, anyhow::Error> {
        let events = self
            .substate_store
            .with_read_tx(|tx| tx.get_events(substate_id, topic, resource_address, offset, limit))?;

        debug!(target: LOG_TARGET, "Found {} events", events.len());
        Ok(events)
    }
}
