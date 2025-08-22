//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, Seq};
use tari_common_types::types::FixedHash;
use tari_consensus_types::Decision;
use tari_engine_types::{
    commit_result::ExecuteResult,
    substate::{SubstateId, SubstateValue},
    template_lib_models::{NonFungibleAddress, ResourceAddress},
    Utxo,
};
use tari_ootle_common_types::{shard::Shard, substate_type::SubstateType, Epoch, StateVersion, VersionedSubstateId};
use tari_ootle_storage::time::PrimitiveDateTime;
use tari_template_abi::TemplateDef;
use tari_template_lib_types::{
    crypto::{RistrettoPublicKeyBytes, UtxoTagByte},
    TemplateAddress,
};
use tari_transaction::{Transaction, TransactionId};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct ListSubstatesRequest {
    pub filter_by_template: Option<TemplateAddress>,
    pub filter_by_type: Option<SubstateType>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct ListSubstatesResponse {
    pub substates: Vec<ListSubstateItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
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
    ts(
        export,
        export_to = "../../bindings/src/types/tari-indexer-client/",
        rename = "IndexerGetSubstateRequest"
    )
)]
pub struct GetSubstateRequest {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub address: SubstateId,
    pub version: Option<u32>,
    #[serde(default)]
    pub local_search_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(
        export,
        export_to = "../../bindings/src/types/tari-indexer-client/",
        rename = "IndexerGetSubstateResponse"
    )
)]
pub struct GetSubstateResponse {
    pub address: SubstateId,
    pub version: u32,
    pub substate: SubstateValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct InspectSubstateRequest {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub address: SubstateId,
    pub version: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct InspectSubstateResponse {
    pub address: SubstateId,
    pub version: u32,
    pub substate: SubstateValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(
        export,
        export_to = "../../bindings/src/types/tari-indexer-client/",
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
        export_to = "../../bindings/src/types/tari-indexer-client/",
        rename = "IndexerSubmitTransactionResponse"
    )
)]
pub struct SubmitTransactionResponse {
    pub transaction_id: TransactionId,
    pub result: IndexerTransactionFinalizedResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct ListTemplatesRequest {
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct ListTemplatesResponse {
    pub templates: Vec<TemplateMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/validator-node-client/")
)]
pub struct TemplateMetadata {
    pub name: String,
    pub address: TemplateAddress,
    /// SHA hash of binary
    pub binary_sha: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(
        export,
        export_to = "../../bindings/src/types/tari-indexer-client/",
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
        export_to = "../../bindings/src/types/tari-indexer-client/",
        rename = "IndexerGetTransactionResultResponse"
    )
)]
pub struct GetTransactionResultResponse {
    pub result: IndexerTransactionFinalizedResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct ListRecentTransactionsRequest {
    pub limit: Option<u32>,
    #[serde(default)]
    pub last_id: Option<TransactionId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct ListRecentTransactionsResponse {
    pub transactions: Vec<TransactionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct TransactionEntry {
    pub transaction_id: TransactionId,
    pub transaction: Transaction,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub created_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
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
    ts(
        export,
        export_to = "../../bindings/src/types/tari-indexer-client/",
        rename = "IndexerGetIdentityResponse"
    )
)]
pub struct GetIdentityResponse {
    pub peer_id: String,
    pub public_key: RistrettoPublicKeyBytes,
    #[cfg_attr(feature = "ts", ts(type = "Array<string>"))]
    pub public_addresses: Vec<Multiaddr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct GetNonFungiblesRequest {
    pub address: ResourceAddress,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub start_index: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub end_index: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct GetNonFungiblesResponse {
    pub non_fungibles: Vec<NonFungibleSubstate>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct NonFungibleSubstate {
    pub address: NonFungibleAddress,
    pub version: u32,
    pub substate: SubstateValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(
        export,
        export_to = "../../bindings/src/types/tari-indexer-client/",
        rename = "IndexerAddPeerRequest"
    )
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
    ts(
        export,
        export_to = "../../bindings/src/types/tari-indexer-client/",
        rename = "IndexerAddPeerResponse"
    )
)]
pub struct AddPeerResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(
        export,
        export_to = "../../bindings/src/types/tari-indexer-client/",
        rename = "IndexerGetCommsStatsResponse"
    )
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
        export_to = "../../bindings/src/types/tari-indexer-client/",
        rename = "IndexerGetEpochManagerStatsResponse"
    )
)]
pub struct GetEpochManagerStatsResponse {
    pub current_epoch: Epoch,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub current_block_height: u64,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub current_block_hash: FixedHash,
}

#[derive(Serialize, Debug)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(
        export,
        export_to = "../../bindings/src/types/tari-indexer-client/",
        rename = "IndexerConnection"
    )
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

#[derive(Serialize, Debug)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(
        export,
        export_to = "../../bindings/src/types/tari-indexer-client/",
        rename = "IndexerConnectionDirection"
    )
)]
pub enum ConnectionDirection {
    Inbound,
    Outbound,
}

#[derive(Serialize, Debug)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(
        export,
        export_to = "../../bindings/src/types/tari-indexer-client/",
        rename = "IndexerGetConnectionsResponse"
    )
)]
pub struct GetConnectionsResponse {
    pub connections: Vec<Connection>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct GetTemplateDefinitionRequest {
    pub template_address: TemplateAddress,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct GetTemplateDefinitionResponse {
    pub name: String,
    pub definition: TemplateDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct IndexerReadyResponse {}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct GetUtxoUpdatesRequest {
    #[cfg_attr(feature = "ts", ts(type = "[[Shard, [StateVersion, UtxoTagByte]]]"))]
    #[serde_as(as = "Seq<(_, _)>")]
    pub shard_state_versions: HashMap<Shard, StateVersion>,
    pub filter_tag_bytes: HashSet<UtxoTagByte>,
    pub resource_address: ResourceAddress,
    pub per_shard_limit: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct GetUtxoUpdatesResponse {
    pub utxo_updates: Vec<UtxoUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub enum UtxoUpdate {
    Unspent(UtxoUnspent),
    Spent(UtxoSpent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct UtxoUnspent {
    pub versioned_substate_id: VersionedSubstateId,
    pub shard: Shard,
    pub state_version: StateVersion,
    pub utxo: Utxo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/tari-indexer-client/")
)]
pub struct UtxoSpent {
    pub versioned_substate_id: VersionedSubstateId,
    pub state_version: StateVersion,
}
