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

use std::{collections::HashMap, fmt::Display, ops::Deref};

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
use tari_engine_types::{ConvertFromByteType, ToByteType};
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
    GetNetworkSyncStateResponse,
    GetNonFungiblesRequest,
    GetNonFungiblesResponse,
    GetSubstateRequest,
    GetSubstateResponse,
    GetSubstatesRequest,
    GetSubstatesResponse,
    GetTemplateDefinitionRequest,
    GetTemplateDefinitionResponse,
    GetTransactionResultRequest,
    GetTransactionResultResponse,
    GetUnspentUtxosRequest,
    GetUnspentUtxosResponse,
    GetUtxoUpdatesRequest,
    GetUtxoUpdatesResponse,
    IndexerReadyResponse,
    IndexerTransactionFinalizedResult,
    InspectSubstateRequest,
    InspectSubstateResponse,
    ListRecentTransactionsRequest,
    ListRecentTransactionsResponse,
    ListSubstatesRequest,
    ListSubstatesResponse,
    ListTemplatesRequest,
    ListTemplatesResponse,
    NetworkDescription,
    SubmitTransactionRequest,
    SubmitTransactionResponse,
    SyncProgress,
    TemplateMetadata,
};
use tari_networking::{is_supported_multiaddr, NetworkingHandle, NetworkingService};
use tari_ootle_app_utilities::keypair::RistrettoKeypair;
use tari_ootle_common_types::{
    optional::Optional,
    public_key_to_peer_id,
    NumPreshards,
    PeerAddress,
    SubstateRequirement,
};
use tari_ootle_p2p::TariMessagingSpec;
use tari_ootle_storage::{
    global::{GlobalDb, TemplateStatus},
    time::{PrimitiveDateTime, UtcDateTime},
};
use tari_ootle_storage_sqlite::global::SqliteGlobalDbAdapter;
use tari_ootle_wallet_sdk::models::{UtxoStateUpdateSet, UtxoUpdateSet};
use tari_template_manager::{
    implementation::TemplateManager,
    interface::{TemplateExecutable, TemplateManagerError},
};
use tari_validator_node_rpc::client::{SubstateResult, TariValidatorNodeRpcClientFactory, TransactionResultStatus};

use crate::{
    bootstrap::Services,
    dry_run::processor::DryRunTransactionProcessor,
    json_rpc::error::internal_error,
    network_client::NetworkClientError,
    storage_sqlite::SqliteIndexerStore,
    substate_manager::SubstateManager,
    transaction_manager::{error::TransactionManagerError, TransactionManager},
};

const LOG_TARGET: &str = "tari::indexer::json_rpc::handlers";

pub struct JsonRpcHandlers {
    keypair: RistrettoKeypair,
    networking: NetworkingHandle<TariMessagingSpec>,
    substate_manager: SubstateManager,
    epoch_manager: EpochManagerHandle<PeerAddress>,
    transaction_manager:
        TransactionManager<EpochManagerHandle<PeerAddress>, TariValidatorNodeRpcClientFactory, SqliteIndexerStore>,
    template_manager: TemplateManager<PeerAddress>,
    global_db: GlobalDb<SqliteGlobalDbAdapter<PeerAddress>>,
    dry_run_transaction_processor: DryRunTransactionProcessor,
}

impl JsonRpcHandlers {
    pub fn new(
        services: &Services,
        substate_manager: SubstateManager,
        transaction_manager: TransactionManager<
            EpochManagerHandle<PeerAddress>,
            TariValidatorNodeRpcClientFactory,
            SqliteIndexerStore,
        >,
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
            value.id.clone(),
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
            .map_err(internal_error(answer_id.clone()))?;
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
                answer_id.clone(),
                JsonRpcError::new(
                    JsonRpcErrorReason::InvalidParams,
                    format!("Unsupported multiaddr {unsupported}"),
                    json::Value::Null,
                ),
            ));
        }

        let mut networking = self.networking.clone();
        let public_key = RistrettoPublicKey::convert_from_byte_type(&public_key).map_err(|_| {
            JsonRpcResponse::error(
                answer_id.clone(),
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
            .map_err(internal_error(answer_id.clone()))?;

        if wait_for_dial {
            dial_wait.await.map_err(internal_error(answer_id.clone()))?;
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
            .map_err(internal_error(answer_id.clone()))?;

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
            .map_err(internal_error(answer_id.clone()))?;

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
            .get_stored_substates_by_filters(filter_by_type, filter_by_template, limit, offset)
            .map_err(|e| {
                warn!(target: LOG_TARGET, "Error getting substate: {}", e);
                Self::internal_error(answer_id.clone(), format!("Error getting substate: {}", e))
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
                Self::internal_error(answer_id.clone(), format!("Error getting substate: {}", e))
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
                                answer_id.clone(),
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

    pub async fn get_substates(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let req: GetSubstatesRequest = value.parse_params()?;

        const MAX_REQUESTS: usize = 20;

        let GetSubstatesRequest { requests } = req;

        if requests.len() > MAX_REQUESTS {
            return Err(Self::invalid_params(
                answer_id,
                format!("Cannot request more than {MAX_REQUESTS} substates at once"),
            ));
        }

        let substates = self.substate_manager.get_substates(requests.as_slice()).map_err(|e| {
            warn!(target: LOG_TARGET, "Error getting substate: {}", e);
            Self::internal_error(answer_id.clone(), format!("Error getting substate: {}", e))
        })?;

        Ok(JsonRpcResponse::success(answer_id, GetSubstatesResponse { substates }))
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
                Self::internal_error(answer_id.clone(), format!("Error getting substate: {}", e))
            })?
            .ok_or_else(|| {
                JsonRpcResponse::error(
                    answer_id.clone(),
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
                answer_id.clone(),
                JsonRpcError::new(
                    JsonRpcErrorReason::InvalidParams,
                    format!("Invalid end_index: {}", e),
                    json::Value::Null,
                ),
            )
        })?;
        let offset = usize::try_from(request.start_index).map_err(|e| {
            JsonRpcResponse::error(
                answer_id.clone(),
                JsonRpcError::new(
                    JsonRpcErrorReason::InvalidParams,
                    format!("Invalid start_index: {}", e),
                    json::Value::Null,
                ),
            )
        })?;

        let non_fungibles = self
            .substate_manager
            .get_non_fungibles_by_resource_address(request.address, limit, offset)
            .map_err(|e| Self::internal_error(answer_id.clone(), format!("Error getting non fungibles: {}", e)))?;
        Ok(JsonRpcResponse::success(answer_id, GetNonFungiblesResponse {
            non_fungibles,
        }))
    }

    pub async fn get_utxo_updates(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let req: GetUtxoUpdatesRequest = value.parse_params()?;
        if req.per_shard_limit > 1000 {
            return Err(JsonRpcResponse::error(
                answer_id,
                JsonRpcError::new(
                    JsonRpcErrorReason::InvalidParams,
                    "per_shard_limit cannot be greater than 1000".to_string(),
                    Value::Null,
                ),
            ));
        }

        if req.shard_state_versions.len() > NumPreshards::MAX_SHARD.as_u32() as usize {
            return Err(JsonRpcResponse::error(
                answer_id,
                JsonRpcError::new(
                    JsonRpcErrorReason::InvalidParams,
                    format!(
                        "shard_state_versions cannot contain more than {} entries",
                        NumPreshards::MAX_SHARD.as_u32()
                    ),
                    Value::Null,
                ),
            ));
        }

        if req
            .shard_state_versions
            .keys()
            .any(|shard| shard.is_global() || *shard > NumPreshards::MAX_SHARD)
        {
            return Err(JsonRpcResponse::error(
                answer_id.clone(),
                JsonRpcError::new(
                    JsonRpcErrorReason::InvalidParams,
                    format!(
                        "shard_state_versions contains invalid shard. Cannot query UTXOs in global shard or greater \
                         than max shard {} exceeded",
                        NumPreshards::MAX_SHARD.as_u32()
                    ),
                    Value::Null,
                ),
            ));
        }

        let mut utxo_updates = HashMap::new();
        let mut per_shard_high_watermark = Vec::with_capacity(req.shard_state_versions.len());
        for (shard, state_version) in req.shard_state_versions {
            let (max_state_version, updates) = self
                .substate_manager
                .get_utxo_updates(req.resource_address, shard, state_version, req.per_shard_limit)
                .map_err(|e| {
                    Self::internal_error(
                        answer_id.clone(),
                        format!(
                            "Error getting UTXO updates for resource_address {}, shard {}, state_version {}: {}",
                            req.resource_address, shard, state_version, e
                        ),
                    )
                })?;

            // TODO: this is on the hot path, figure out a better way to let the client know the max shard versions
            // without sending it each time
            let current_tip_version = self
                .substate_manager
                .get_max_state_version(&req.resource_address, shard)
                .map_err(|e| {
                    Self::internal_error(
                        answer_id.clone(),
                        format!(
                            "Error getting max state version for resource_address {}, shard {}: {}",
                            req.resource_address, shard, e
                        ),
                    )
                })?;
            if current_tip_version.as_u64() > 0 {
                // Save a little over the wire initially by not sending 0 watermarks
                per_shard_high_watermark.push((shard, current_tip_version));
            }
            if !updates.is_empty() {
                utxo_updates.insert(shard, UtxoStateUpdateSet {
                    updates,
                    max_state_version,
                });
            }
        }

        Ok(JsonRpcResponse::success(answer_id, GetUtxoUpdatesResponse {
            updates: UtxoUpdateSet {
                shard_updates: utxo_updates,
                per_shard_high_watermark,
            },
        }))
    }

    pub async fn get_unspent_utxos(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let req: GetUnspentUtxosRequest = value.parse_params()?;
        if req.tag_and_nonce_pairs.len() > 1000 {
            return Err(JsonRpcResponse::error(
                answer_id,
                JsonRpcError::new(
                    JsonRpcErrorReason::InvalidParams,
                    "cannot query more than 1000 UTXOs".to_string(),
                    Value::Null,
                ),
            ));
        }
        let utxos = self
            .substate_manager
            .get_unspent_utxos(&req.resource_address, &req.tag_and_nonce_pairs)
            .map_err(|e| {
                Self::internal_error(
                    answer_id.clone(),
                    format!(
                        "Error getting UTXOs for resource_address {}, with {} tag/nonce pair(s): {}",
                        req.resource_address,
                        req.tag_and_nonce_pairs.len(),
                        e
                    ),
                )
            })?;

        Ok(JsonRpcResponse::success(answer_id, GetUnspentUtxosResponse { utxos }))
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
                .map_err(|e| Self::internal_error(answer_id.clone(), e))?;

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
                        answer_id.clone(),
                        JsonRpcError::new(
                            JsonRpcErrorReason::ApplicationError(400),
                            format!("All validators failed: {}", e),
                            json::Value::Null,
                        ),
                    )
                },
                TransactionManagerError::InvalidTransaction {
                    transaction_id,
                    details,
                } => JsonRpcResponse::error(
                    answer_id.clone(),
                    JsonRpcError::new(
                        JsonRpcErrorReason::ApplicationError(400),
                        format!("Transaction {} is invalid: {}", transaction_id, details),
                        json::Value::Null,
                    ),
                ),
                e => Self::internal_error(answer_id.clone(), e),
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
            .map_err(internal_error(answer_id.clone()))?;

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
                    answer_id.clone(),
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

    pub async fn get_network_sync_state(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let network_desc = self
            .epoch_manager
            .get_network_description()
            .await
            .map_err(internal_error(answer_id.clone()))?;
        let sync_progress = self
            .substate_manager
            .get_sync_progress()
            .optional()
            .map_err(internal_error(answer_id.clone()))?;

        let response = GetNetworkSyncStateResponse {
            network_desc: NetworkDescription {
                epoch: network_desc.epoch,
                shard_groups: network_desc
                    .shard_groups
                    .into_iter()
                    .map(|(shard_group, info)| (shard_group, info.num_members))
                    .collect(),
                num_preshards: network_desc.num_preshards,
            },
            sync_progress: sync_progress.map(|p| SyncProgress {
                last_epoch: p.last_epoch,
                checkpoint_progress: p.checkpoint_progress.into_iter().collect(),
                last_state_versions: p.last_state_versions.into_iter().collect(),
            }),
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
            .map_err(|err| {
                // If it's pending, we return a 404 to the client - this allows them to retry later
                if matches!(err, TemplateManagerError::TemplateUnavailable {
                    status: Some(TemplateStatus::Pending)
                }) {
                    Self::not_found(
                        answer_id.clone(),
                        format!(
                            "Template with address {} is still being downloaded. Try again later.",
                            request.template_address
                        ),
                    )
                } else {
                    Self::internal_error(answer_id.clone(), format!("Error fetching template: {}", err))
                }
            })?
            .ok_or_else(|| {
                Self::not_found(
                    answer_id.clone(),
                    format!("Template with address {} not found", request.template_address),
                )
            })?;
        let template = match template.executable {
            TemplateExecutable::CompiledWasm(code) => WasmModule::from_code(code)
                .load_template()
                .map_err(|e| Self::internal_error(answer_id.clone(), format!("Error loading template: {}", e)))?,
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
            .map_err(|e| Self::internal_error(answer_id.clone(), e))?;

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
            .map_err(|e| Self::internal_error(answer_id.clone(), e))?
            .ok_or_else(|| Self::not_found(answer_id.clone(), "Transaction not found"))?;

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

    pub async fn list_recent_transactions(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        let req: ListRecentTransactionsRequest = value.parse_params()?;
        if req.limit.is_some_and(|l| l > 1000) {
            return Err(JsonRpcResponse::error(
                answer_id,
                JsonRpcError::new(
                    JsonRpcErrorReason::InvalidParams,
                    "Limit cannot be greater than 1000".to_string(),
                    json::Value::Null,
                ),
            ));
        }

        let limit = req.limit.unwrap_or(100);
        if limit > 1000 {
            return Err(JsonRpcResponse::error(
                answer_id.clone(),
                JsonRpcError::new(
                    JsonRpcErrorReason::InvalidParams,
                    "Limit cannot be greater than 1000".to_string(),
                    json::Value::Null,
                ),
            ));
        }

        let transactions = self
            .transaction_manager
            .list_recent_transactions(None, limit as usize)
            .map_err(|e| Self::internal_error(answer_id.clone(), e))?;

        let resp = ListRecentTransactionsResponse { transactions };
        Ok(JsonRpcResponse::success(answer_id, resp))
    }

    pub async fn wait_until_ready(&self, value: JsonRpcExtractor) -> JrpcResult {
        let answer_id = value.get_answer_id();
        // TODO: we should rather return the current state of the indexer and have clients poll
        self.epoch_manager
            .wait_for_initial_scanning_to_complete()
            .await
            .map_err(internal_error(answer_id.clone()))?;

        Ok(JsonRpcResponse::success(answer_id, IndexerReadyResponse {}))
    }

    fn error_response<T: Display>(answer_id: axum_jrpc::Id, reason: JsonRpcErrorReason, message: T) -> JsonRpcResponse {
        JsonRpcResponse::error(
            answer_id,
            JsonRpcError::new(reason, message.to_string(), json::Value::Null),
        )
    }

    fn not_found<T: Display>(answer_id: axum_jrpc::Id, details: T) -> JsonRpcResponse {
        Self::error_response(answer_id, JsonRpcErrorReason::ApplicationError(404), details)
    }

    fn invalid_params<T: Display>(answer_id: axum_jrpc::Id, details: T) -> JsonRpcResponse {
        Self::error_response(answer_id, JsonRpcErrorReason::InvalidParams, details)
    }

    fn internal_error<T: Display>(answer_id: axum_jrpc::Id, error: T) -> JsonRpcResponse {
        error!(target: LOG_TARGET, "Internal error: {}", error);
        Self::error_response(answer_id, JsonRpcErrorReason::InternalError, error)
    }
}

fn now() -> PrimitiveDateTime {
    let now = UtcDateTime::now();
    PrimitiveDateTime::new(now.date(), now.time())
}
