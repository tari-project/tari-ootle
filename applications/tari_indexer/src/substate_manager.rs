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

use std::{convert::TryInto, sync::Arc};

use serde::{Deserialize, Serialize};
use tari_common_types::types::FixedHash;
use tari_engine_types::substate::{Substate, SubstateId, SubstateValue};
use tari_epoch_manager::service::EpochManagerHandle;
use tari_indexer_client::types::ListSubstateItem;
use tari_indexer_lib::{substate_scanner::SubstateScanner, NonFungibleSubstate};
use tari_ootle_app_utilities::substate_file_cache::SubstateFileCache;
use tari_ootle_common_types::{substate_type::SubstateType, PeerAddress, VersionedSubstateIdRef};
use tari_template_lib::{models::ResourceAddress, types::TemplateAddress};
use tari_validator_node_rpc::client::{SubstateResult, TariValidatorNodeRpcClientFactory};

use crate::substate_storage_sqlite::sqlite_substate_store_factory::{
    SqliteSubstateStore,
    SubstateStore,
    SubstateStoreReadTransaction,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct SubstateResponse {
    pub address: SubstateId,
    pub version: u32,
    pub substate: SubstateValue,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NonFungibleResponse {
    pub index: u64,
    pub address: SubstateId,
    pub substate: Substate,
}

impl From<NonFungibleSubstate> for NonFungibleResponse {
    fn from(nf: NonFungibleSubstate) -> Self {
        Self {
            index: nf.index,
            address: nf.address,
            substate: nf.substate,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EventResponse {
    pub address: SubstateId,
    pub created_by_transaction: FixedHash,
}

pub struct SubstateManager {
    substate_scanner:
        Arc<SubstateScanner<EpochManagerHandle<PeerAddress>, TariValidatorNodeRpcClientFactory, SubstateFileCache>>,
    substate_store: SqliteSubstateStore,
}

impl SubstateManager {
    pub fn new(
        substate_scanner: Arc<
            SubstateScanner<EpochManagerHandle<PeerAddress>, TariValidatorNodeRpcClientFactory, SubstateFileCache>,
        >,
        substate_store: SqliteSubstateStore,
    ) -> Self {
        Self {
            substate_scanner,
            substate_store,
        }
    }

    pub async fn list_substates(
        &self,
        filter_by_type: Option<SubstateType>,
        filter_by_template: Option<TemplateAddress>,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<ListSubstateItem>, anyhow::Error> {
        let mut tx = self.substate_store.create_read_tx()?;
        let substates = tx.list_substates(filter_by_type, filter_by_template, limit, offset)?;
        Ok(substates)
    }

    pub async fn get_substate(
        &self,
        substate_address: &SubstateId,
        version: Option<u32>,
    ) -> Result<Option<SubstateResponse>, anyhow::Error> {
        // we store the latest version of the substates related to the events
        // so we will return the substate directly from database if it's there
        if let Some(substate) = self.get_substate_from_db(substate_address, version).await? {
            return Ok(Some(substate));
        }

        // the substate is not in db (or is not the requested version) so we fetch it from the dan layer committee
        let substate_result = self.substate_scanner.get_substate(substate_address, version).await?;
        match substate_result {
            SubstateResult::Up { id, substate } => Ok(Some(SubstateResponse {
                address: id,
                version: substate.version(),
                substate: substate.into_substate_value(),
            })),
            _ => Ok(None),
        }
    }

    async fn get_substate_from_db(
        &self,
        substate_address: &SubstateId,
        version: Option<u32>,
    ) -> Result<Option<SubstateResponse>, anyhow::Error> {
        let mut tx = self.substate_store.create_read_tx()?;
        if let Some(row) = tx.get_substate(substate_address, version)? {
            // the substate is present in db and the version matches the requested version
            let substate_resp = row.try_into()?;
            return Ok(Some(substate_resp));
        };

        // the substate is not present in db
        Ok(None)
    }

    pub async fn get_specific_substate(
        &self,
        versioned_id: VersionedSubstateIdRef<'_>,
    ) -> Result<SubstateResult, anyhow::Error> {
        let substate_result = self
            .substate_scanner
            .get_specific_substate_from_committee(versioned_id.into())
            .await?;
        Ok(substate_result)
    }

    pub fn get_non_fungibles_by_resource_address(
        &self,
        address: ResourceAddress,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<NonFungibleSubstate>, anyhow::Error> {
        let mut tx = self.substate_store.create_read_tx()?;
        let nfts = tx.get_non_fungibles_by_resource_address(address, limit, offset)?;
        Ok(nfts)
    }

    pub async fn get_non_fungible_count(&self, substate_id: &SubstateId) -> Result<u64, anyhow::Error> {
        if !substate_id.is_resource() {
            return Err(anyhow::anyhow!(
                "get_non_fungible_count must be called with resource address, got {substate_id}"
            ));
        }

        let address_str = substate_id.to_address_string();
        let mut tx = self.substate_store.create_read_tx()?;
        let count = tx.get_non_fungible_count(address_str)?;
        Ok(count as u64)
    }
}
