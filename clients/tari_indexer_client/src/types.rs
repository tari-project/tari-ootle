//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, time::Duration};

use bounded_vec::BoundedVec;
use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};
use tari_common_types::types::FixedHash;
use tari_consensus_types::Decision;
use tari_engine_types::{
    commit_result::ExecuteResult,
    substate::{Substate, SubstateId, SubstateValue},
    template_lib_models::{NonFungibleAddress, ResourceAddress, UtxoId},
    transaction_receipt::{TransactionReceipt, TransactionReceiptAddress},
    Utxo,
};
use tari_ootle_common_types::{
    shard::Shard,
    substate_type::SubstateType,
    Epoch,
    NumPreshards,
    ShardGroup,
    StateVersion,
};
use tari_ootle_storage::{time::PrimitiveDateTime, Ordering};
use tari_ootle_wallet_sdk::models::UtxoUpdateSet;
use tari_template_abi::TemplateDef;
use tari_template_lib_types::{
    crypto::{RistrettoPublicKeyBytes, UtxoTag},
    TemplateAddress,
};
use tari_transaction::{Transaction, TransactionId};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct ListSubstatesRequest {
    pub filter_by_template: Option<TemplateAddress>,
    pub filter_by_type: Option<SubstateType>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct ListSubstatesResponse {
    pub substates: Vec<ListSubstateItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct ListSubstateItem {
    pub substate_id: SubstateId,
    pub module_name: Option<String>,
    pub version: u32,
    pub template_address: Option<TemplateAddress>,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub timestamp: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tari-indexer-client/", rename = "IndexerGetSubstateRequest")
)]
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
pub struct GetSubstateResponse {
    pub version: u32,
    pub substate: SubstateValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct GetSubstatesRequest {
    // Note that we may permit less than 50 in the handler, but this is the max we'll deserialize for DoS mitigation
    #[cfg_attr(feature = "ts", ts(as = "Vec<SubstateId>"))]
    pub requests: BoundedVec<SubstateId, 1, 50>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct GetSubstatesResponse {
    pub substates: HashMap<SubstateId, Substate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct InspectSubstateRequest {
    pub address: SubstateId,
    pub version: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct InspectSubstateResponse {
    pub version: u32,
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
pub struct SubmitTransactionRequest {
    pub transaction: Transaction,
    pub is_dry_run: bool,
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
pub struct SubmitTransactionResponse {
    pub transaction_id: TransactionId,
    pub result: IndexerTransactionFinalizedResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct ListTemplatesRequest {
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct ListTemplatesResponse {
    pub templates: Vec<TemplateMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct TemplateMetadata {
    pub name: String,
    pub address: TemplateAddress,
    /// SHA hash of binary
    #[cfg_attr(feature = "ts", ts(type = "number[]"))]
    pub binary_sha: FixedHash,
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
pub struct GetTransactionResultRequest {
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
pub struct GetTransactionResultResponse {
    pub result: IndexerTransactionFinalizedResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct ListRecentTransactionsRequest {
    pub limit: Option<u32>,
    #[serde(default)]
    pub last_id: Option<TransactionId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct ListRecentTransactionsResponse {
    pub transactions: Vec<TransactionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct TransactionEntry {
    pub transaction_id: TransactionId,
    pub transaction: Transaction,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub created_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub enum IndexerTransactionFinalizedResult {
    Pending,
    Finalized {
        final_decision: Decision,
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
pub struct GetIdentityResponse {
    pub peer_id: String,
    pub public_key: RistrettoPublicKeyBytes,
    #[cfg_attr(feature = "ts", ts(type = "Array<string>"))]
    pub public_addresses: Vec<Multiaddr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct GetNonFungiblesRequest {
    pub address: ResourceAddress,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub start_index: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub end_index: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct GetNonFungiblesResponse {
    pub non_fungibles: Vec<NonFungibleSubstate>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct NonFungibleSubstate {
    pub address: NonFungibleAddress,
    pub version: u32,
    pub substate: SubstateValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tari-indexer-client/", rename = "IndexerAddPeerRequest")
)]
pub struct AddPeerRequest {
    pub public_key: RistrettoPublicKeyBytes,
    #[cfg_attr(feature = "ts", ts(type = "Array<string>"))]
    pub addresses: Vec<Multiaddr>,
    pub wait_for_dial: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tari-indexer-client/", rename = "IndexerAddPeerResponse")
)]
pub struct AddPeerResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tari-indexer-client/", rename = "IndexerGetCommsStatsResponse")
)]
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
pub struct GetEpochManagerStatsResponse {
    pub current_epoch: Epoch,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub current_block_height: u64,
    #[cfg_attr(feature = "ts", ts(type = "number[]"))]
    pub current_block_hash: FixedHash,
}

#[derive(Serialize, Deserialize, Debug)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tari-indexer-client/", rename = "IndexerConnection")
)]
pub struct Connection {
    pub connection_id: String,
    pub peer_id: String,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub address: Multiaddr,
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
pub struct GetConnectionsResponse {
    pub connections: Vec<Connection>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct GetTemplateDefinitionRequest {
    pub template_address: TemplateAddress,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct GetTemplateDefinitionResponse {
    pub name: String,
    pub definition: TemplateDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct IndexerReadyResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct GetUtxoUpdatesRequest {
    #[serde(default)]
    pub from_epoch: Epoch,
    pub shard_state_versions: Vec<(Shard, StateVersion)>,
    pub resource_address: ResourceAddress,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unspent_only: bool,
    pub per_shard_limit: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct GetUtxoUpdatesResponse {
    pub updates: UtxoUpdateSet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct GetUtxosRequest {
    pub tag_and_nonce_pairs: Vec<(UtxoTag, RistrettoPublicKeyBytes)>,
    pub resource_address: ResourceAddress,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct GetUtxosResponse {
    pub utxos: Vec<(UtxoId, Utxo)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct ListUtxosRequest {
    pub resource_address: ResourceAddress,
    pub limit: u32,
    pub from_id: Option<UtxoId>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct ListUtxosResponse {
    pub utxos: Vec<(UtxoId, Utxo)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct GetNetworkSyncStateResponse {
    pub network_desc: NetworkDescription,
    pub sync_progress: Option<SyncProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct NetworkDescription {
    pub epoch: Epoch,
    // (shard group, num members)
    pub shard_groups: Vec<(ShardGroup, u32)>,
    pub num_preshards: NumPreshards,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct SyncProgress {
    pub last_epoch: Epoch,
    pub checkpoint_progress: Vec<(ShardGroup, Epoch)>,
    pub last_state_versions: Vec<(Shard, (StateVersion, Epoch))>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct ListTransactionReceiptsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_id: Option<TransactionReceiptAddress>,
    #[serde(default)]
    pub ordering: Ordering,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct ListTransactionReceiptsResponse {
    pub receipts: Vec<(TransactionReceiptAddress, TransactionReceipt)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "tari-indexer-client/"))]
pub struct GetTransactionReceiptResponse {
    pub receipt: TransactionReceipt,
}
