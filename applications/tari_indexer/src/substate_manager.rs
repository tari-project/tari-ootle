//  Copyright 2023, The Tari Project
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

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use tari_engine_types::{
    substate::{Substate, SubstateId, SubstateValue},
    Utxo,
};
use tari_epoch_manager::service::EpochManagerHandle;
use tari_indexer_client::types::{ListSubstateItem, NonFungibleSubstate};
use tari_indexer_lib::{cached_substate_manager::CachedSubstateManager, error::IndexerError};
use tari_ootle_common_types::{shard::Shard, substate_type::SubstateType, Epoch, PeerAddress, StateVersion};
use tari_ootle_storage::StorageError;
use tari_ootle_wallet_sdk::models::UtxoStateUpdateSet;
use tari_template_lib::{
    models::{ResourceAddress, UtxoId},
    types::{
        crypto::{RistrettoPublicKeyBytes, UtxoTag},
        TemplateAddress,
    },
};
use tari_validator_node_rpc::client::{SubstateResult, TariValidatorNodeRpcClientFactory};

use crate::{
    storage_sqlite::SqliteIndexerStore,
    store::{IndexerStoreReadTransaction, IndexerStoreReader},
    substate_file_cache::SubstateFileCache,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct SubstateResponse {
    pub id: SubstateId,
    pub version: u32,
    pub substate: SubstateValue,
}

#[derive(Debug, Clone)]
pub struct SubstateManager {
    cached_substate:
        CachedSubstateManager<EpochManagerHandle<PeerAddress>, TariValidatorNodeRpcClientFactory, SubstateFileCache>,
    substate_store: SqliteIndexerStore,
}

impl SubstateManager {
    pub fn new(
        cached_substate: CachedSubstateManager<
            EpochManagerHandle<PeerAddress>,
            TariValidatorNodeRpcClientFactory,
            SubstateFileCache,
        >,
        substate_store: SqliteIndexerStore,
    ) -> Self {
        Self {
            cached_substate,
            substate_store,
        }
    }

    pub fn get_stored_substates_by_filters(
        &self,
        by_id: Option<&SubstateId>,
        filter_by_type: Option<SubstateType>,
        filter_by_template: Option<TemplateAddress>,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<ListSubstateItem>, SubstateManagerError> {
        let substates = self
            .substate_store
            .with_read_tx(|tx| tx.list_substates(by_id, filter_by_type, filter_by_template, limit, offset))?;
        Ok(substates)
    }

    pub fn get_utxo_updates(
        &self,
        resource_address: ResourceAddress,
        from_epoch: Epoch,
        shard: Shard,
        from_state_version: StateVersion,
        unspent_only: bool,
        limit: u32,
    ) -> Result<UtxoStateUpdateSet, SubstateManagerError> {
        let updates = self.substate_store.with_read_tx(|tx| {
            tx.utxos_get_updates(
                resource_address,
                from_epoch,
                shard,
                from_state_version,
                unspent_only,
                limit,
            )
        })?;
        Ok(updates)
    }

    pub fn get_max_state_version(
        &self,
        resource_address: &ResourceAddress,
        shard: Shard,
    ) -> Result<StateVersion, SubstateManagerError> {
        let max_version = self
            .substate_store
            .with_read_tx(|tx| tx.utxos_get_max_state_version(*resource_address, shard))?;
        Ok(max_version)
    }

    pub fn get_unspent_utxos(
        &self,
        resource_address: &ResourceAddress,
        public_nonce_and_tag: &[(UtxoTag, RistrettoPublicKeyBytes)],
    ) -> Result<Vec<(UtxoId, Utxo)>, SubstateManagerError> {
        let utxos = self
            .substate_store
            .with_read_tx(|tx| tx.utxos_get_unspent_by_public_nonce_and_tag(resource_address, public_nonce_and_tag))?;
        Ok(utxos)
    }

    pub fn list_utxos(
        &self,
        resource_address: &ResourceAddress,
        from_id: Option<UtxoId>,
        limit: u32,
    ) -> Result<Vec<(UtxoId, Utxo)>, SubstateManagerError> {
        let utxos = self
            .substate_store
            .with_read_tx(|tx| tx.utxos_list(resource_address, from_id, limit))?;
        Ok(utxos)
    }

    pub async fn get_substate(
        &self,
        substate_id: &SubstateId,
        version: Option<u32>,
    ) -> Result<Option<SubstateResponse>, SubstateManagerError> {
        if let Some(version) = version {
            if let Some(substate) = self.get_substate_from_db(substate_id, Some(version))? {
                return Ok(Some(substate));
            }
        }

        let substate_result = self.cached_substate.get_substate(substate_id, version).await?;
        match substate_result {
            SubstateResult::Up { substate } => Ok(Some(SubstateResponse {
                id: substate_id.clone(),
                version: substate.version(),
                substate: substate.into_substate_value(),
            })),
            _ => Ok(None),
        }
    }

    pub async fn get_cached_substates(
        &self,
        substates: &[SubstateId],
    ) -> Result<HashMap<SubstateId, Substate>, SubstateManagerError> {
        let mut substate_set = substates.iter().collect::<HashSet<_>>();

        let mut substates = self.substate_store.with_read_tx(|tx| tx.get_substates(substates))?;

        for k in substates.keys() {
            substate_set.remove(k);
        }

        let cached = self
            .cached_substate
            .get_cached_substates(substate_set.into_iter())
            .await?;
        for (id, substate) in cached {
            let Some(substate) = substate else {
                continue;
            };
            let Some(substate) = substate.substate_result.into_up() else {
                continue;
            };
            substates.insert(id.clone(), substate);
        }

        Ok(substates)
    }

    fn get_substate_from_db(
        &self,
        substate_address: &SubstateId,
        version: Option<u32>,
    ) -> Result<Option<SubstateResponse>, SubstateManagerError> {
        let mut tx = self.substate_store.create_read_tx()?;
        if let Some(row) = tx.get_substate(substate_address, version)? {
            // the substate is present in db and the version matches the requested version
            let substate_resp = row.try_into()?;
            return Ok(Some(substate_resp));
        };

        // the substate is not present in db
        Ok(None)
    }

    pub fn get_non_fungibles_by_resource_address(
        &self,
        address: ResourceAddress,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<NonFungibleSubstate>, SubstateManagerError> {
        let mut tx = self.substate_store.create_read_tx()?;
        let nfts = tx.get_non_fungibles_by_resource_address(address, limit, offset)?;
        Ok(nfts)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SubstateManagerError {
    #[error("Indexer error: {0}")]
    IndexerError(#[from] IndexerError),
    #[error("Storage error: {0}")]
    StorageError(#[from] StorageError),
}
