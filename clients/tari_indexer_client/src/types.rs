//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, time::Duration};

use bounded_vec::BoundedVec;
use serde::{Deserialize, Serialize};
use tari_consensus_types::Decision;
use tari_engine_types::{
    Utxo,
    commit_result::ExecuteResult,
    events::Event,
    resource::Resource,
    substate::{Substate, SubstateId, SubstateValue},
    transaction_receipt::TransactionReceipt,
};
use tari_ootle_common_types::{Epoch, Network, NumPreshards, ShardGroup, StateVersion, shard::Shard};
use tari_ootle_transaction::{Transaction, TransactionEnvelope, TransactionId};
use tari_template_abi::TemplateDef;
use tari_template_lib_types::{
    Amount,
    Hash32,
    NonFungibleAddress,
    ResourceAddress,
    TemplateAddress,
    TransactionReceiptAddress,
    UtxoId,
    crypto::{RistrettoPublicKeyBytes, UtxoTag},
};
use time::PrimitiveDateTime;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ListSubstateItem {
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub substate_id: SubstateId,
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<String>))]
    pub module_name: Option<String>,
    pub version: u32,
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<String>))]
    pub template_address: Option<TemplateAddress>,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub timestamp: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tari-indexer-client/", rename = "IndexerGetSubstateRequest")
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetSubstateRequest {
    pub version: Option<u32>,
    #[serde(default)]
    pub local_search_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tari-indexer-client/", rename = "IndexerGetSubstateResponse")
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetSubstateResponse {
    pub version: u32,
    #[cfg_attr(feature = "utoipa", schema(value_type = Object))]
    pub substate: SubstateValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetSubstatesRequest {
    // Note that we may permit less than 50 in the handler, but this is the max we'll deserialize for DoS mitigation
    /// The list of substate IDs to fetch
    #[cfg_attr(feature = "ts", ts(as = "Vec<SubstateId>"))]
    #[cfg_attr(feature = "utoipa", schema(value_type = Vec<String>))]
    pub requests: BoundedVec<SubstateId, 1, 50>,
    /// If true, only search local storage for the substates. This may result in substates not being found even if they
    /// exist. Otherwise, the indexer will attempt to fetch substates from validator nodes across various shard groups
    /// which may result in more failures.
    pub cached_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetSubstatesResponse {
    #[cfg_attr(feature = "utoipa", schema(value_type = HashMap<String, Object>))]
    pub substates: HashMap<SubstateId, Substate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct InspectSubstateRequest {
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub address: SubstateId,
    pub version: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct InspectSubstateResponse {
    pub version: u32,
    #[cfg_attr(feature = "utoipa", schema(value_type = Object))]
    pub substate: SubstateValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(
        export,
        export_to = "tari-indexer-client/",
        rename = "IndexerSubmitTransactionRequest"
    )
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct SubmitTransactionRequest {
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    /// A BOR-encoded transaction envelope, base64 encoded as a string
    pub transaction: TransactionEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(
        export,
        export_to = "tari-indexer-client/",
        rename = "IndexerSubmitTransactionResponse"
    )
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct SubmitTransactionResponse {
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    /// The ID of the submitted transaction
    pub transaction_id: TransactionId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(
        export,
        export_to = "tari-indexer-client/",
        rename = "IndexerSubmitTransactionResponse"
    )
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct SubmitTransactionDryRunResponse {
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    /// The ID of the transaction that was dry-run
    pub transaction_id: TransactionId,
    #[cfg_attr(feature = "utoipa", schema(value_type = Object))]
    /// The result of the dry-run execution, including any emitted events and state changes, but without a final
    /// decision or commitment to the ledger
    pub result: ExecuteResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ListTemplatesRequest {
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ListTemplatesResponse {
    pub templates: Vec<TemplateMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct TemplateMetadata {
    pub name: String,
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub address: TemplateAddress,
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub binary_sha: Hash32,
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub author_public_key: RistrettoPublicKeyBytes,
    pub code_size: usize,
    #[cfg_attr(feature = "utoipa", schema(value_type = u64))]
    pub epoch: Epoch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(
        export,
        export_to = "tari-indexer-client/",
        rename = "IndexerGetTransactionResultRequest"
    )
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetTransactionResultRequest {
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    /// The ID of the transaction to query the result for
    pub transaction_id: TransactionId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(
        export,
        export_to = "tari-indexer-client/",
        rename = "IndexerGetTransactionResultResponse"
    )
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetTransactionResultResponse {
    #[cfg_attr(feature = "utoipa", schema(value_type = Object))]
    /// The result of the transaction, which may be pending (not yet finalized) or finalized with details such as the
    /// final decision, execution result, and timestamps
    pub result: IndexerTransactionFinalizedResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct QueryTransactionEventsRequest {
    /// Filter events by topic
    pub topic: Option<String>,
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<String>))]
    pub substate_id: Option<SubstateId>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct QueryTransactionEventsResponse {
    #[cfg_attr(feature = "utoipa", schema(value_type = Vec<(String, Object)>))]
    pub events: Vec<(TransactionId, Event)>,
}

/// Filter parameters for the transaction events SSE stream.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct StreamTransactionEventsRequest {
    pub topic: Option<String>,
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<String>))]
    pub substate_id: Option<SubstateId>,
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<String>))]
    pub template_address: Option<TemplateAddress>,
    /// Resume the event stream from this event ID (exclusive). Events with id > after_id will be
    /// replayed from the database before switching to the live stream.
    pub after_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ListRecentTransactionsRequest {
    pub limit: Option<u32>,
    #[serde(default)]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<String>))]
    pub last_id: Option<TransactionId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ListRecentTransactionsResponse {
    pub transactions: Vec<TransactionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct TransactionEntry {
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub transaction_id: TransactionId,
    #[cfg_attr(feature = "utoipa", schema(value_type = Object))]
    pub transaction: Transaction,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub created_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum IndexerTransactionFinalizedResult {
    Pending,
    Finalized {
        #[cfg_attr(feature = "utoipa", schema(value_type = String))]
        final_decision: Decision,
        #[cfg_attr(feature = "utoipa", schema(value_type = Option<Object>))]
        execution_result: Option<Box<ExecuteResult>>,
        #[cfg_attr(feature = "ts", ts(type = "{secs: number, nanos: number}"))]
        execution_time: Duration,
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        finalized_time: PrimitiveDateTime,
        abort_details: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tari-indexer-client/", rename = "IndexerGetIdentityResponse")
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetIdentityResponse {
    pub peer_id: String,
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub public_key: RistrettoPublicKeyBytes,
    pub public_addresses: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetNonFungiblesRequest {
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub address: ResourceAddress,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub start_index: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub end_index: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetNonFungiblesResponse {
    pub non_fungibles: Vec<NonFungibleSubstate>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct NonFungibleSubstate {
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub address: NonFungibleAddress,
    pub version: u32,
    #[cfg_attr(feature = "utoipa", schema(value_type = Object))]
    pub substate: SubstateValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tari-indexer-client/", rename = "IndexerGetCommsStatsResponse")
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetCommsStatsResponse {
    pub connection_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(
        export,
        export_to = "tari-indexer-client/",
        rename = "IndexerGetEpochManagerStatsResponse"
    )
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetEpochManagerStatsResponse {
    #[cfg_attr(feature = "utoipa", schema(value_type = u64))]
    /// The current epoch according to the indexer's epoch oracle view
    pub current_epoch: Epoch,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub current_block_height: u64,
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub current_block_hash: Hash32,
}

#[derive(Serialize, Deserialize, Debug)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tari-indexer-client/", rename = "IndexerConnection")
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct Connection {
    pub connection_id: String,
    pub peer_id: String,
    pub address: String,
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub direction: ConnectionDirection,
    #[cfg_attr(feature = "ts", ts(type = "{secs: number, nanos: number}"))]
    pub age: Duration,
    #[cfg_attr(feature = "ts", ts(type = "{secs: number, nanos: number} | null"))]
    pub ping_latency: Option<Duration>,
    pub user_agent: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tari-indexer-client/", rename = "IndexerConnectionDirection")
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum ConnectionDirection {
    Inbound,
    Outbound,
}

#[derive(Serialize, Deserialize, Debug)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tari-indexer-client/", rename = "IndexerGetConnectionsResponse")
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetConnectionsResponse {
    pub connections: Vec<Connection>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetTemplateDefinitionResponse {
    pub name: String,
    #[cfg_attr(feature = "utoipa", schema(value_type = Object))]
    pub definition: TemplateDef,
    pub code_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct IndexerReadyResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetUtxoUpdatesRequest {
    #[serde(default)]
    #[cfg_attr(feature = "utoipa", schema(value_type = u64))]
    pub from_epoch: Epoch,
    #[cfg_attr(feature = "utoipa", schema(value_type = (u32, u64)))]
    pub shard_state_versions: Vec<(Shard, StateVersion)>,
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub resource_address: ResourceAddress,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unspent_only: bool,
    pub per_shard_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoUpdateSet {
    pub shard_updates: HashMap<Shard, UtxoStateUpdateSet>,
    pub per_shard_high_watermark: Vec<(Shard, StateVersion)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoStateUpdateSet {
    pub updates: Vec<WalletUtxoUpdate>,
    pub max_state_version: StateVersion,
    pub max_epoch: Epoch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum WalletUtxoUpdate {
    Unspent(UtxoUnspent),
    Spent(UtxoSpent),
    Burnt(UtxoBurnt),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoUnspent {
    pub tag: UtxoTag,
    pub public_nonce: RistrettoPublicKeyBytes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoSpent {
    pub id: UtxoId,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoBurnt {
    pub id: UtxoId,
    pub version: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetUtxoUpdatesResponse {
    #[cfg_attr(feature = "utoipa", schema(value_type = Object))]
    pub updates: UtxoUpdateSet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetUtxosRequest {
    #[cfg_attr(feature = "utoipa", schema(value_type = Vec<(u32, String)>))]
    pub tag_and_nonce_pairs: Vec<(UtxoTag, RistrettoPublicKeyBytes)>,
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub resource_address: ResourceAddress,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetUtxosResponse {
    #[cfg_attr(feature = "utoipa", schema(value_type = Vec<(String, Object)>))]
    pub utxos: Vec<(UtxoId, Utxo)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ListUtxosRequest {
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub resource_address: ResourceAddress,
    pub limit: u32,
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<String>))]
    pub from_id: Option<UtxoId>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ListUtxosResponse {
    #[cfg_attr(feature = "utoipa", schema(value_type = Vec<(String, Object)>))]
    pub utxos: Vec<(UtxoId, Utxo)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetNetworkInfoResponse {
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub network: Network,
    pub network_byte: u8,
    #[cfg_attr(feature = "utoipa", schema(value_type = u64))]
    pub epoch: Epoch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetNetworkSyncStateResponse {
    pub network_desc: NetworkDescription,
    pub sync_progress: Option<SyncProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct NetworkDescription {
    #[cfg_attr(feature = "utoipa", schema(value_type = u64))]
    pub epoch: Epoch,
    // (shard group, num members)
    #[cfg_attr(feature = "utoipa", schema(value_type = Vec<(Object, u32)>))]
    pub shard_groups: Vec<(ShardGroup, u32)>,
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub num_preshards: NumPreshards,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct SyncProgress {
    #[cfg_attr(feature = "utoipa", schema(value_type = u64))]
    pub last_epoch: Epoch,
    #[cfg_attr(feature = "utoipa", schema(value_type = Vec<(Object, u64)>))]
    pub checkpoint_progress: Vec<(ShardGroup, Epoch)>,
    #[cfg_attr(feature = "utoipa", schema(value_type = Vec<(u32, (u64, u64))>))]
    pub last_state_versions: Vec<(Shard, (StateVersion, Epoch))>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ListTransactionReceiptsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<String>))]
    pub last_id: Option<TransactionReceiptAddress>,
    #[serde(default)]
    pub ordering: Ordering,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum Ordering {
    // Use default only where you "don't care" about the order. Ascending is more performant so it's the default.
    #[default]
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ListTransactionReceiptsResponse {
    #[cfg_attr(feature = "utoipa", schema(value_type = Vec<(String, Object)>))]
    pub receipts: Vec<(TransactionReceiptAddress, TransactionReceipt)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetTransactionReceiptResponse {
    #[cfg_attr(feature = "utoipa", schema(value_type = Object))]
    pub receipt: TransactionReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetResourceResponse {
    #[cfg_attr(feature = "utoipa", schema(value_type = Object))]
    pub resource: Resource,
    pub version: u32,
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<String>))]
    pub total_supply: Option<Amount>,
}
