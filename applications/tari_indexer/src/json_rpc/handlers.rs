//   Copyright 2023. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{collections::HashMap, fmt::Display, ops::Deref, sync::Arc};

use axum_jrpc::{
    error::{JsonRpcError, JsonRpcErrorReason},
    JrpcResult,
    JsonRpcExtractor,
    JsonRpcResponse,
};
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use log::{error, info, warn};
use serde_json::{self as json, json, Value};
use tari_consensus_types::Decision;
use tari_crypto::{ristretto::RistrettoPublicKey, tari_utilities::hex::to_hex};
use tari_engine::{template::TemplateModuleLoader, wasm::WasmModule};
use tari_engine_types::{FromByteType, ToByteType};
use tari_epoch_manager::{service::EpochManagerHandle, EpochManagerReader};
use tari_epoch_oracles::store::StoreKey;
use tari_indexer_client::types::{
    self,
    AddPeerRequest,
    AddPeerResponse,
    ConnectionDirection,
    GetCommsStatsResponse,
    GetConnectionsResponse,
    GetEpochManagerStatsResponse,
    GetIdentityResponse,
    GetNonFungiblesRequest,
    GetNonFungiblesResponse,
    GetSubstateRequest,
    GetSubstateResponse,
    GetTemplateDefinitionRequest,
    GetTemplateDefinitionResponse,
    GetTransactionResultRequest,
    GetTransactionResultResponse,
    IndexerReadyResponse,
    IndexerTransactionFinalizedResult,
    InspectSubstateRequest,
    InspectSubstateResponse,
    ListSubstatesRequest,
    ListSubstatesResponse,
    ListTemplatesRequest,
    ListTemplatesResponse,
    NonFungibleSubstate,
    SubmitTransactionRequest,
    SubmitTransactionResponse,
    TemplateMetadata,
};
use tari_networking::{is_supported_multiaddr, NetworkingHandle, NetworkingService};
use tari_ootle_app_utilities::keypair::RistrettoKeypair;
use tari_ootle_common_types::{optional::Optional, public_key_to_peer_id, PeerAddress, SubstateRequirement};
use tari_ootle_p2p::TariMessagingSpec;
use tari_ootle_storage::{
    global::GlobalDb,
    time::{PrimitiveDateTime, UtcDateTime},
};
use tari_ootle_storage_sqlite::global::SqliteGlobalDbAdapter;
use tari_template_manager::{implementation::TemplateManager, interface::TemplateExecutable};
use tari_validator_node_rpc::client::{SubstateResult, TariValidatorNodeRpcClientFactory, TransactionResultStatus};

use crate::{
    bootstrap::Services,
    dry_run::processor::DryRunTransactionProcessor,
    json_rpc::error::internal_error,
    network_client::NetworkClientError,
    substate_manager::SubstateManager,
    transaction_manager::{error::TransactionManagerError, TransactionManager},
};

const LOG_TARGET: &str = "tari::indexer::json_rpc::handlers";

pub struct JsonRpcHandlers {
    keypair: RistrettoKeypair,
    networking: NetworkingHandle<TariMessagingSpec>,
    substate_manager: Arc<SubstateManager>,
    epoch_manager: EpochManagerHandle<PeerAddress>,
    transaction_manager: TransactionManager<EpochManagerHandle<PeerAddress>, TariValidatorNodeRpcClientFactory>,
    template_manager: TemplateManager<PeerAddress>,
    global_db: GlobalDb<SqliteGlobalDbAdapter<PeerAddress>>,
    dry_run_transaction_processor: DryRunTransactionProcessor,
}

impl JsonRpcHandlers {
    pub fn new(
        services: &Services,
        substate_manager: Arc<SubstateManager>,
        transaction_manager: TransactionManager<EpochManagerHandle<PeerAddress>, TariValidatorNodeRpcClientFactory>,
        global_db: GlobalDb<SqliteGlobalDbAdapter<PeerAddress>>,
        template_manager: TemplateManager<PeerAddress>,
        dry_run_transaction_processor: DryRunTransactionProcessor,
    ) -> Self {
        Self {
            keypair: services.keypair.clone(),
            networking: services.networking.clone(),
            global_db,
            substate_manager,
            epoch_manager: services.epoch_manager.clone(),
            transaction_manager,
            template_manager,
            dry_run_transaction_processor,
        }
    }
}

impl JsonRpcHandlers {
    pub fn rpc_discover(&self, value: JsonRpcExtractor) -> JrpcResult {
        Ok(JsonRpcResponse::success(
            value.id,
            serde_json::from_str::<HashMap<String, Value>>(include_str!("../../openrpc.json")).map_err(|e| {
                JsonRpcResponse::error(
                    value.id,
                    JsonRpcError::new(JsonRpcErrorReason::InternalError, e.to_string(), json!({})),
                )
            })?,
        ))
    }

    pub async fn get_identity(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let info = self
            .networking
            .get_local_peer_info()
            .await
            .map_err(internal_error(answer_id))?;
        let response = GetIdentityResponse {
            peer_id: info.peer_id.to_string(),
            public_key: self.keypair.public_key().to_byte_type(),
            public_addresses: info.listen_addrs,
        };

        Ok(JsonRpcResponse::success(answer_id, response))
    }

    pub async fn add_peer(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let AddPeerRequest {
            public_key,
            addresses,
            wait_for_dial,
        } = value.parse_params()?;

        if let Some(unsupported) = addresses.iter().find(|a| !is_supported_multiaddr(a)) {
            return Err(JsonRpcResponse::error(
                answer_id,
                JsonRpcError::new(
                    JsonRpcErrorReason::InvalidParams,
                    format!("Unsupported multiaddr {unsupported}"),
                    json::Value::Null,
                ),
            ));
        }

        let mut networking = self.networking.clone();
        let public_key = RistrettoPublicKey::try_from_byte_type(&public_key).map_err(|_| {
            JsonRpcResponse::error(
                answer_id,
                JsonRpcError::new(
                    JsonRpcErrorReason::InvalidParams,
                    "Public key is malformed".to_string(),
                    json::Value::Null,
                ),
            )
        })?;
        let peer_id = public_key_to_peer_id(public_key);

        let dial_wait = networking
            .dial_peer(
                DialOpts::peer_id(peer_id)
                    .addresses(addresses)
                    .condition(PeerCondition::Always)
                    .build(),
            )
            .await
            .map_err(internal_error(answer_id))?;

        if wait_for_dial {
            dial_wait.await.map_err(internal_error(answer_id))?;
        }

        Ok(JsonRpcResponse::success(answer_id, AddPeerResponse {}))
    }

    pub async fn get_comms_stats(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let peers = self
            .networking
            .clone()
            .get_connected_peers()
            .await
            .map_err(internal_error(answer_id))?;

        let status = if peers.is_empty() { "Offline" } else { "Online" };
        Ok(JsonRpcResponse::success(answer_id, GetCommsStatsResponse {
            connection_status: status.to_string(),
        }))
    }

    pub async fn get_connections(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let active_connections = self
            .networking
            .get_active_connections()
            .await
            .map_err(internal_error(answer_id))?;

        let connections = active_connections
            .into_iter()
            .map(|conn| types::Connection {
                connection_id: conn.connection_id.to_string(),
                peer_id: conn.peer_id.to_string(),
                address: conn.endpoint.get_remote_address().clone(),
                direction: if conn.endpoint.is_dialer() {
                    ConnectionDirection::Outbound
                } else {
                    ConnectionDirection::Inbound
                },
                age: conn.age(),
                ping_latency: conn.ping_latency,
                user_agent: conn.user_agent.map(|arc| arc.deref().clone()),
            })
            .collect();

        Ok(JsonRpcResponse::success(answer_id, GetConnectionsResponse {
            connections,
        }))
    }

    pub async fn list_substates(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let ListSubstatesRequest {
            filter_by_template,
            filter_by_type,
            limit,
            offset,
        } = value.parse_params()?;

        let substates = self
            .substate_manager
            .list_substates(filter_by_type, filter_by_template, limit, offset)
            .await
            .map_err(|e| {
                warn!(target: LOG_TARGET, "Error getting substate: {}", e);
                Self::internal_error(answer_id, format!("Error getting substate: {}", e))
            })?;

        Ok(JsonRpcResponse::success(answer_id, ListSubstatesResponse { substates }))
    }

    pub async fn get_substate(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let request: GetSubstateRequest = value.parse_params()?;

        let maybe_substate = self
            .substate_manager
            .get_substate(&request.address, request.version)
            .await
            .map_err(|e| {
                warn!(target: LOG_TARGET, "Error getting substate: {}", e);
                Self::internal_error(answer_id, format!("Error getting substate: {}", e))
            })?;

        match maybe_substate {
            Some(substate_resp) => Ok(JsonRpcResponse::success(answer_id, GetSubstateResponse {
                address: substate_resp.address,
                version: substate_resp.version,
                substate: substate_resp.substate,
            })),
            None => {
                if request.local_search_only {
                    Err(JsonRpcResponse::error(
                        answer_id,
                        JsonRpcError::new(
                            JsonRpcErrorReason::ApplicationError(404),
                            format!(
                                "Substate {} (version:>={}) not found",
                                request.address,
                                request.version.unwrap_or(0)
                            ),
                            Value::Null,
                        ),
                    ))
                } else {
                    // Ask network
                    let substate = self
                        .transaction_manager
                        .get_substate(&SubstateRequirement::new(request.address.clone(), request.version))
                        .await
                        .map_err(|e| {
                            warn!(target: LOG_TARGET, "Error asking network for substate: {}", e);
                            JsonRpcResponse::error(
                                answer_id,
                                JsonRpcError::new(
                                    JsonRpcErrorReason::ApplicationError(501),
                                    format!("Error asking network for substate:{}", e),
                                    Value::Null,
                                ),
                            )
                        })?;
                    match substate {
                        SubstateResult::DoesNotExist => Err(JsonRpcResponse::error(
                            answer_id,
                            JsonRpcError::new(
                                JsonRpcErrorReason::ApplicationError(404),
                                format!(
                                    "Substate {} (version:>={}) not found, and not found on network",
                                    request.address,
                                    request.version.unwrap_or(0)
                                ),
                                Value::Null,
                            ),
                        )),
                        SubstateResult::Up { id, substate } => {
                            Ok(JsonRpcResponse::success(answer_id, GetSubstateResponse {
                                address: id,
                                version: substate.version(),
                                substate: substate.into_substate_value(),
                            }))
                        },
                        SubstateResult::Down { version, .. } => Err(JsonRpcResponse::error(
                            answer_id,
                            JsonRpcError::new(
                                JsonRpcErrorReason::ApplicationError(301),
                                format!(
                                    "Substate {} (version:>={}) not found, but found in a down state on network at \
                                     version {}",
                                    request.address,
                                    request.version.unwrap_or(0) + 1,
                                    version
                                ),
                                Value::Null,
                            ),
                        )),
                    }
                }
            },
        }
    }

    pub async fn inspect_substate(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let request: InspectSubstateRequest = value.parse_params()?;

        let resp = self
            .substate_manager
            .get_substate(&request.address, request.version)
            .await
            .map_err(|e| {
                warn!(target: LOG_TARGET, "Error getting substate: {}", e);
                Self::internal_error(answer_id, format!("Error getting substate: {}", e))
            })?
            .ok_or_else(|| {
                JsonRpcResponse::error(
                    answer_id,
                    JsonRpcError::new(
                        JsonRpcErrorReason::ApplicationError(404),
                        format!(
                            "Substate {} (version:>={}) not found",
                            request.address,
                            request.version.unwrap_or(0)
                        ),
                        Value::Null,
                    ),
                )
            })?;

        Ok(JsonRpcResponse::success(answer_id, InspectSubstateResponse {
            address: resp.address,
            version: resp.version,
            substate: resp.substate,
        }))
    }

    pub async fn get_non_fungibles(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let request: GetNonFungiblesRequest = value.parse_params()?;
        let limit = usize::try_from(request.end_index.saturating_sub(request.start_index)).map_err(|e| {
            JsonRpcResponse::error(
                answer_id,
                JsonRpcError::new(
                    JsonRpcErrorReason::InvalidParams,
                    format!("Invalid end_index: {}", e),
                    json::Value::Null,
                ),
            )
        })?;
        let offset = usize::try_from(request.start_index).map_err(|e| {
            JsonRpcResponse::error(
                answer_id,
                JsonRpcError::new(
                    JsonRpcErrorReason::InvalidParams,
                    format!("Invalid start_index: {}", e),
                    json::Value::Null,
                ),
            )
        })?;

        let res = self
            .substate_manager
            .get_non_fungibles_by_resource_address(request.address, limit, offset)
            .map_err(|e| Self::internal_error(answer_id, format!("Error getting non fungibles: {}", e)))?;
        Ok(JsonRpcResponse::success(answer_id, GetNonFungiblesResponse {
            non_fungibles: res
                .into_iter()
                .map(|v| NonFungibleSubstate {
                    index: v.index,
                    address: v.address,
                    substate: v.substate,
                })
                .collect(),
        }))
    }

    pub async fn submit_transaction(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let request: SubmitTransactionRequest = value.parse_params()?;

        if request.is_dry_run {
            let transaction_id = request.transaction.calculate_id();
            let exec_result = self
                .dry_run_transaction_processor
                .process_transaction(request.transaction)
                .await
                .map_err(|e| Self::internal_error(answer_id, e))?;

            return Ok(JsonRpcResponse::success(answer_id, SubmitTransactionResponse {
                result: IndexerTransactionFinalizedResult::Finalized {
                    execution_result: Some(Box::new(exec_result)),
                    final_decision: Decision::Commit,
                    abort_details: None,
                    finalized_time: now(),
                    execution_time: Default::default(),
                },
                transaction_id,
            }));
        }

        let transaction = request.transaction;
        let transaction_id = self
            .transaction_manager
            .submit_transaction(transaction)
            .await
            .map_err(|e| match e {
                TransactionManagerError::NetworkClientError(NetworkClientError::AllValidatorsFailed { .. }) => {
                    JsonRpcResponse::error(
                        answer_id,
                        JsonRpcError::new(
                            JsonRpcErrorReason::ApplicationError(400),
                            format!("All validators failed: {}", e),
                            json::Value::Null,
                        ),
                    )
                },
                e => Self::internal_error(answer_id, e),
            })?;

        info!(target: LOG_TARGET, "✅ Transaction submitted: {}", transaction_id);

        Ok(JsonRpcResponse::success(answer_id, SubmitTransactionResponse {
            result: IndexerTransactionFinalizedResult::Pending,
            transaction_id,
        }))
    }

    pub async fn get_epoch_manager_stats(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let current_epoch = self.epoch_manager.get_current_epoch();
        let current_epoch_hash = self
            .epoch_manager
            .get_current_epoch_hash()
            .await
            .map_err(internal_error(answer_id))?;

        let current_block_height = self
            .global_db
            .create_transaction()
            .and_then(|mut tx| {
                self.global_db
                    .metadata(&mut tx)
                    .get_metadata::<u64>(StoreKey::BaseLayerLastScannedBlockHeight.as_key_bytes())
            })
            .map_err(|e| {
                JsonRpcResponse::error(
                    answer_id,
                    JsonRpcError::new(
                        JsonRpcErrorReason::InternalError,
                        format!("Could not get current block height: {}", e),
                        json::Value::Null,
                    ),
                )
            })?;

        let response = GetEpochManagerStatsResponse {
            current_epoch,
            current_block_height: current_block_height.unwrap_or(0),
            current_block_hash: current_epoch_hash,
        };
        Ok(JsonRpcResponse::success(answer_id, response))
    }

    pub async fn get_template_definition(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let request: GetTemplateDefinitionRequest = value.parse_params()?;

        let template = self
            .template_manager
            .fetch_template(&request.template_address)
            .optional()
            .map_err(|e| Self::internal_error(answer_id, e))?
            .ok_or_else(|| {
                Self::not_found(
                    answer_id,
                    format!("Template with address {} not found", request.template_address),
                )
            })?;
        let template = match template.executable {
            TemplateExecutable::CompiledWasm(code) => WasmModule::from_code(code)
                .load_template()
                .map_err(|e| Self::internal_error(answer_id, format!("Error loading template: {}", e)))?,
            // TemplateExecutable::DownloadableWasm is never returned ad there is no DB type for that
            TemplateExecutable::DownloadableWasm(_, _) | TemplateExecutable::Manifest(_) => {
                return Err(JsonRpcResponse::error(
                    answer_id,
                    JsonRpcError::new(
                        JsonRpcErrorReason::InvalidRequest,
                        "Template is not a wasm module".to_string(),
                        json::Value::Null,
                    ),
                ));
            },
        };

        Ok(JsonRpcResponse::success(answer_id, GetTemplateDefinitionResponse {
            definition: template.template_def().clone(),
            name: template.template_name().to_string(),
        }))
    }

    pub async fn list_templates(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let req: ListTemplatesRequest = value.parse_params()?;
        let templates = self
            .template_manager
            .fetch_template_metadata(req.limit as usize)
            .map_err(|e| Self::internal_error(answer_id, e))?;

        Ok(JsonRpcResponse::success(answer_id, ListTemplatesResponse {
            templates: templates
                .into_iter()
                .map(|t| TemplateMetadata {
                    name: t.name,
                    address: t.address,
                    binary_sha: to_hex(t.binary_sha.as_slice()),
                })
                .collect(),
        }))
    }

    pub async fn get_transaction_result(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let request: GetTransactionResultRequest = value.parse_params()?;

        let result = self
            .transaction_manager
            .get_transaction_result(request.transaction_id)
            .await
            .optional()
            .map_err(|e| Self::internal_error(answer_id, e))?
            .ok_or_else(|| Self::not_found(answer_id, "Transaction not found"))?;

        let resp = match result {
            TransactionResultStatus::Pending => GetTransactionResultResponse {
                result: IndexerTransactionFinalizedResult::Pending,
            },
            TransactionResultStatus::Finalized(finalized) => GetTransactionResultResponse {
                result: IndexerTransactionFinalizedResult::Finalized {
                    final_decision: finalized.final_decision,
                    execution_result: finalized.execute_result.map(Box::new),
                    execution_time: finalized.execution_time,
                    finalized_time: finalized.finalized_time,
                    abort_details: finalized.abort_details,
                },
            },
        };

        Ok(JsonRpcResponse::success(answer_id, resp))
    }

    pub async fn wait_until_ready(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        // TODO: we should rather return the current state of the indexer and have clients poll
        self.epoch_manager
            .wait_for_initial_scanning_to_complete()
            .await
            .map_err(internal_error(answer_id))?;

        Ok(JsonRpcResponse::success(answer_id, IndexerReadyResponse {}))
    }

    fn error_response<T: Display>(answer_id: i64, reason: JsonRpcErrorReason, message: T) -> JsonRpcResponse {
        JsonRpcResponse::error(
            answer_id,
            JsonRpcError::new(reason, message.to_string(), json::Value::Null),
        )
    }

    fn not_found<T: Display>(answer_id: i64, details: T) -> JsonRpcResponse {
        Self::error_response(answer_id, JsonRpcErrorReason::ApplicationError(404), details)
    }

    fn internal_error<T: Display>(answer_id: i64, error: T) -> JsonRpcResponse {
        error!(target: LOG_TARGET, "Internal error: {}", error);
        Self::error_response(answer_id, JsonRpcErrorReason::InternalError, error)
    }
}

fn now() -> PrimitiveDateTime {
    let now = UtcDateTime::now();
    PrimitiveDateTime::new(now.date(), now.time())
}
