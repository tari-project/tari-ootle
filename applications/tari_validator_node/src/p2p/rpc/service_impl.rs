//  Copyright 2021, The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that
// the  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the
// following  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED
// WARRANTIES,  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
// PARTICULAR PURPOSE ARE  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY
// DIRECT, INDIRECT, INCIDENTAL,  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
// PROCUREMENT OF SUBSTITUTE GOODS OR  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY,  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
// OTHERWISE) ARISING IN ANY WAY OUT OF THE  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH
// DAMAGE.
use std::convert::{TryFrom, TryInto};

use log::*;
use tari_bor::encode;
use tari_dan_app_utilities::template_manager::interface::TemplateManagerHandle;
use tari_dan_common_types::{optional::Optional, shard::Shard, Epoch, NodeHeight, PeerAddress, SubstateRequirement};
use tari_dan_p2p::{
    proto,
    proto::rpc::{
        GetCheckpointRequest,
        GetCheckpointResponse,
        GetHighQcRequest,
        GetHighQcResponse,
        GetSubstateRequest,
        GetSubstateResponse,
        GetTransactionResultRequest,
        GetTransactionResultResponse,
        PayloadResultStatus,
        SubstateStatus,
        SyncBlocksRequest,
        SyncBlocksResponse,
        SyncStateRequest,
        SyncStateResponse,
        SyncTemplatesRequest,
        SyncTemplatesResponse,
    },
};
use tari_dan_storage::{
    consensus_models::{Block, BlockId, EpochCheckpoint, HighQc, StateTransitionId, SubstateRecord, TransactionRecord},
    StateStore,
};
use tari_engine_types::TemplateAddress;
use tari_epoch_manager::{base_layer::EpochManagerHandle, EpochManagerReader};
use tari_rpc_framework::{Request, Response, RpcStatus, Streaming};
use tari_state_store_sqlite::SqliteStateStore;
use tari_template_lib::HashParseError;
use tari_transaction::{Transaction, TransactionId};
use tari_validator_node_rpc::rpc_service::ValidatorNodeRpcService;
use tokio::{sync::mpsc, task};

use crate::{
    consensus::ConsensusHandle,
    p2p::{
        rpc::{block_sync_task::BlockSyncTask, state_sync_task::StateSyncTask, template_sync_task::TemplateSyncTask},
        services::mempool::MempoolHandle,
    },
};

const LOG_TARGET: &str = "tari::dan::p2p::rpc";

pub struct ValidatorNodeRpcServiceImpl {
    epoch_manager: EpochManagerHandle<PeerAddress>,
    template_manager: TemplateManagerHandle,
    shard_state_store: SqliteStateStore<PeerAddress>,
    mempool: MempoolHandle,
    consensus: ConsensusHandle,
}

impl ValidatorNodeRpcServiceImpl {
    pub fn new(
        epoch_manager: EpochManagerHandle<PeerAddress>,
        template_manager: TemplateManagerHandle,
        shard_state_store: SqliteStateStore<PeerAddress>,
        mempool: MempoolHandle,
        consensus: ConsensusHandle,
    ) -> Self {
        Self {
            epoch_manager,
            template_manager,
            shard_state_store,
            mempool,
            consensus,
        }
    }
}

#[tari_rpc_framework::async_trait]
impl ValidatorNodeRpcService for ValidatorNodeRpcServiceImpl {
    async fn submit_transaction(
        &self,
        request: Request<proto::rpc::SubmitTransactionRequest>,
    ) -> Result<Response<proto::rpc::SubmitTransactionResponse>, RpcStatus> {
        let request = request.into_message();
        let transaction: Transaction = request
            .transaction
            .ok_or_else(|| RpcStatus::bad_request("Missing transaction"))?
            .try_into()
            .map_err(|e| RpcStatus::bad_request(format!("Malformed transaction: {}", e)))?;

        let transaction_id = *transaction.id();
        info!(target: LOG_TARGET, "🌐 Received transaction {transaction_id} from peer");

        self.mempool
            .submit_transaction(transaction)
            .await
            .map_err(|e| RpcStatus::bad_request(format!("Invalid transaction: {}", e)))?;

        debug!(target: LOG_TARGET, "Accepted transaction {transaction_id} into mempool");

        Ok(Response::new(proto::rpc::SubmitTransactionResponse {
            transaction_id: transaction_id.as_bytes().to_vec(),
        }))
    }

    async fn get_substate(&self, req: Request<GetSubstateRequest>) -> Result<Response<GetSubstateResponse>, RpcStatus> {
        let req = req.into_message();

        let substate_requirement = req
            .substate_requirement
            .map(SubstateRequirement::try_from)
            .transpose()
            .map_err(|e| RpcStatus::bad_request(format!("Invalid substate requirement: {e}")))?
            .ok_or_else(|| RpcStatus::bad_request("Missing substate requirement"))?;

        if !substate_requirement.substate_id().is_global() {
            let current_epoch = self
                .epoch_manager
                .current_epoch()
                .await
                .map_err(RpcStatus::log_internal_error(LOG_TARGET))?;
            let local_committee_info = self
                .epoch_manager
                .get_local_committee_info(current_epoch)
                .await
                .map_err(RpcStatus::log_internal_error(LOG_TARGET))?;
            if !local_committee_info.includes_substate_id(substate_requirement.substate_id()) {
                return Err(RpcStatus::bad_request(format!(
                    "This node in {} does not store {}",
                    local_committee_info.shard_group(),
                    substate_requirement
                )));
            }
        }

        debug!(
            target: LOG_TARGET,
            "Querying substate {substate_requirement} from the state store"
        );
        let tx = self
            .shard_state_store
            .create_read_tx()
            .map_err(RpcStatus::log_internal_error(LOG_TARGET))?;

        let maybe_substate = substate_requirement
            .to_substate_address()
            .map(|address| SubstateRecord::get(&tx, &address))
            // Just fetch the latest if no version is supplied as a requirement
            .unwrap_or_else(|| SubstateRecord::get_latest(&tx, substate_requirement.substate_id()))
            .optional()
            .map_err(RpcStatus::log_internal_error(LOG_TARGET))?;

        let Some(substate) = maybe_substate else {
            return Ok(Response::new(GetSubstateResponse {
                status: SubstateStatus::DoesNotExist as i32,
                ..Default::default()
            }));
        };

        let created_qc = substate
            .get_created_quorum_certificate(&tx)
            .map_err(RpcStatus::log_internal_error(LOG_TARGET))?;

        let resp = if substate.is_destroyed() {
            let destroyed_qc = substate
                .get_destroyed_quorum_certificate(&tx)
                .map_err(RpcStatus::log_internal_error(LOG_TARGET))?;
            GetSubstateResponse {
                status: SubstateStatus::Down as i32,
                address: substate.substate_id().to_bytes(),
                version: substate.version(),
                created_transaction_hash: substate.created_by_transaction().into_array().to_vec(),
                destroyed_transaction_hash: substate
                    .destroyed()
                    .map(|destroyed| destroyed.by_transaction.as_bytes().to_vec())
                    .unwrap_or_default(),
                quorum_certificates: Some(created_qc)
                    .into_iter()
                    .chain(destroyed_qc)
                    .map(|qc| (&qc).into())
                    .collect(),
                ..Default::default()
            }
        } else {
            GetSubstateResponse {
                status: SubstateStatus::Up as i32,
                address: substate.substate_id().to_bytes(),
                version: substate.version(),
                substate: substate.substate_value().to_bytes(),
                created_transaction_hash: substate.created_by_transaction().into_array().to_vec(),
                destroyed_transaction_hash: vec![],
                quorum_certificates: vec![(&created_qc).into()],
            }
        };

        Ok(Response::new(resp))
    }

    async fn get_transaction_result(
        &self,
        req: Request<GetTransactionResultRequest>,
    ) -> Result<Response<GetTransactionResultResponse>, RpcStatus> {
        let req = req.into_message();
        let tx = self
            .shard_state_store
            .create_read_tx()
            .map_err(RpcStatus::log_internal_error(LOG_TARGET))?;
        let tx_id = TransactionId::try_from(req.transaction_id)
            .map_err(|_| RpcStatus::bad_request("Invalid transaction id"))?;
        let transaction = TransactionRecord::get(&tx, &tx_id)
            .optional()
            .map_err(RpcStatus::log_internal_error(LOG_TARGET))?
            .ok_or_else(|| RpcStatus::not_found("Transaction not found"))?;

        let Some(final_decision) = transaction.final_decision() else {
            return Ok(Response::new(GetTransactionResultResponse {
                status: PayloadResultStatus::Pending.into(),
                ..Default::default()
            }));
        };

        let abort_details = transaction.abort_reason().map(|r| r.to_string()).unwrap_or_default();

        Ok(Response::new(GetTransactionResultResponse {
            status: PayloadResultStatus::Finalized.into(),

            final_decision: Some(proto::consensus::Decision::from(final_decision)),
            execution_time_ms: transaction
                .execution_time()
                .map(|t| u64::try_from(t.as_millis()).unwrap_or(u64::MAX))
                .unwrap_or_default(),
            finalized_time_ms: transaction
                .finalized_time()
                .map(|t| u64::try_from(t.as_millis()).unwrap_or(u64::MAX))
                .unwrap_or_default(),
            abort_details,
            // For simplicity, we simply encode the whole result as a CBOR blob.
            execution_result: transaction
                .into_final_result()
                .as_ref()
                .map(encode)
                .transpose()
                .map_err(RpcStatus::log_internal_error(LOG_TARGET))?
                .unwrap_or_default(),
        }))
    }

    async fn sync_blocks(
        &self,
        request: Request<SyncBlocksRequest>,
    ) -> Result<Streaming<SyncBlocksResponse>, RpcStatus> {
        let req = request.into_message();
        let store = self.shard_state_store.clone();
        let current_epoch = self
            .epoch_manager
            .current_epoch()
            .await
            .map_err(RpcStatus::log_internal_error(LOG_TARGET))?;

        let start_block_id = Some(req.start_block_id)
            .filter(|i| !i.is_empty())
            .map(BlockId::try_from)
            .transpose()
            .map_err(|e| RpcStatus::bad_request(format!("Invalid encoded block id: {}", e)))?;

        let start_block_id = {
            let tx = store
                .create_read_tx()
                .map_err(RpcStatus::log_internal_error(LOG_TARGET))?;

            match start_block_id {
                Some(id) => {
                    if !Block::record_exists(&tx, &id).map_err(RpcStatus::log_internal_error(LOG_TARGET))? {
                        return Err(RpcStatus::not_found(format!("start_block_id {id} not found",)));
                    }
                    id
                },
                None => {
                    let epoch = req
                        .epoch
                        .map(Epoch::from)
                        .map(|end| end.min(current_epoch))
                        .unwrap_or(current_epoch);

                    let mut block_ids = Block::get_ids_by_epoch_and_height(&tx, epoch, NodeHeight::zero())
                        .map_err(RpcStatus::log_internal_error(LOG_TARGET))?;

                    let Some(block_id) = block_ids.pop() else {
                        return Err(RpcStatus::not_found(format!(
                            "Block not found with epoch={epoch},height=0"
                        )));
                    };
                    if !block_ids.is_empty() {
                        return Err(RpcStatus::conflict(format!(
                            "Multiple applicable blocks for epoch={} and height=0",
                            current_epoch
                        )));
                    }

                    block_id
                },
            }
        };

        let (sender, receiver) = mpsc::channel(10);
        task::spawn(BlockSyncTask::new(self.shard_state_store.clone(), start_block_id, None, sender).run());

        Ok(Streaming::new(receiver))
    }

    async fn get_high_qc(&self, _request: Request<GetHighQcRequest>) -> Result<Response<GetHighQcResponse>, RpcStatus> {
        let current_epoch = self.consensus.current_epoch();
        let high_qc = self
            .shard_state_store
            .with_read_tx(|tx| {
                HighQc::get(tx, current_epoch)
                    .optional()?
                    .map(|hqc| hqc.get_quorum_certificate(tx))
                    .transpose()
            })
            .map_err(RpcStatus::log_internal_error(LOG_TARGET))?;

        Ok(Response::new(GetHighQcResponse {
            high_qc: high_qc.as_ref().map(Into::into),
        }))
    }

    async fn get_checkpoint(
        &self,
        request: Request<GetCheckpointRequest>,
    ) -> Result<Response<GetCheckpointResponse>, RpcStatus> {
        let msg = request.into_message();
        let current_epoch = self.consensus.current_epoch();

        let prev_epoch = current_epoch.saturating_sub(Epoch(1));
        if prev_epoch.is_zero() {
            return Err(RpcStatus::not_found("Cannot generate checkpoint for genesis epoch"));
        }

        if msg.current_epoch != current_epoch {
            // This may occur if one of the nodes has not fully scanned the base layer
            return Err(RpcStatus::bad_request(format!(
                "Peer requested checkpoint with epoch {} but current epoch is {}",
                msg.current_epoch, current_epoch
            )));
        }

        let checkpoint = self
            .shard_state_store
            .with_read_tx(|tx| EpochCheckpoint::get(tx, prev_epoch))
            .optional()
            .map_err(RpcStatus::log_internal_error(LOG_TARGET))?;

        Ok(Response::new(GetCheckpointResponse {
            checkpoint: checkpoint.map(Into::into),
        }))
    }

    async fn sync_state(&self, request: Request<SyncStateRequest>) -> Result<Streaming<SyncStateResponse>, RpcStatus> {
        let req = request.into_message();

        let (sender, receiver) = mpsc::channel(10);

        let start_epoch = Epoch(req.start_epoch);
        let start_shard = Shard::from(req.start_shard);
        let last_state_transition_for_chain = StateTransitionId::new(start_epoch, start_shard, req.start_seq);

        let end_epoch = Epoch(req.current_epoch);
        info!(target: LOG_TARGET, "🌍peer initiated sync with this node ({}, {}, seq={}) to {}", start_epoch, start_shard, req.start_seq, end_epoch);

        task::spawn(
            StateSyncTask::new(
                self.shard_state_store.clone(),
                sender,
                last_state_transition_for_chain,
                end_epoch,
            )
            .run(),
        );

        Ok(Streaming::new(receiver))
    }

    async fn sync_templates(
        &self,
        request: Request<SyncTemplatesRequest>,
    ) -> Result<Streaming<SyncTemplatesResponse>, RpcStatus> {
        let req = request.into_message();

        let (tx, rx) = mpsc::channel(10);
        let addresses = req
            .addresses
            .iter()
            .map(|raw| TemplateAddress::try_from_vec(raw.clone()))
            .collect::<Result<Vec<TemplateAddress>, HashParseError>>()
            .map_err(|error| RpcStatus::bad_request(format!("Failed to parse address: {:?}", error)))?;

        task::spawn(TemplateSyncTask::new(5, addresses, tx, self.template_manager.clone()).run());

        Ok(Streaming::new(rx))
    }
}
