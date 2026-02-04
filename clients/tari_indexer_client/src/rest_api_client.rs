//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use reqwest::{IntoUrl, Url, header, header::HeaderMap};
use serde::{Serialize, de::DeserializeOwned};
use tari_engine_types::substate::SubstateId;
use tari_template_lib_types::{ResourceAddress, TemplateAddress, TransactionReceiptAddress};

use crate::{
    error::IndexerRestClientError,
    protobuf,
    protobuf_stream::ProtobufStream,
    sse::{SseEventStream, SseEventStreamBuilder},
    types::{
        AddPeerRequest,
        AddPeerResponse,
        GetConnectionsResponse,
        GetEpochManagerStatsResponse,
        GetNetworkInfoResponse,
        GetNetworkSyncStateResponse,
        GetNonFungiblesRequest,
        GetNonFungiblesResponse,
        GetResourceResponse,
        GetSubstateRequest,
        GetSubstateResponse,
        GetSubstatesRequest,
        GetSubstatesResponse,
        GetTemplateDefinitionResponse,
        GetTransactionReceiptResponse,
        GetTransactionResultRequest,
        GetTransactionResultResponse,
        GetUtxoUpdatesRequest,
        GetUtxosRequest,
        GetUtxosResponse,
        IndexerReadyResponse,
        ListRecentTransactionsRequest,
        ListRecentTransactionsResponse,
        ListSubstatesRequest,
        ListSubstatesResponse,
        ListTemplatesRequest,
        ListTemplatesResponse,
        ListTransactionReceiptsRequest,
        ListTransactionReceiptsResponse,
        ListUtxosRequest,
        ListUtxosResponse,
        SubmitTransactionDryRunResponse,
        SubmitTransactionRequest,
        SubmitTransactionResponse,
    },
};

#[derive(Debug, Clone)]
pub struct IndexerRestApiClient {
    client: reqwest::Client,
    endpoint: Url,
}

impl IndexerRestApiClient {
    pub fn connect<T: IntoUrl>(endpoint: T) -> Result<Self, IndexerRestClientError> {
        let client = reqwest::Client::builder()
            .default_headers({
                let mut headers = HeaderMap::with_capacity(1);
                headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
                headers
            })
            .build()?;

        Ok(Self {
            client,
            endpoint: endpoint.into_url()?,
        })
    }

    pub async fn get_connections(&self) -> Result<GetConnectionsResponse, IndexerRestClientError> {
        self.send_get("network/connections", ()).await
    }

    pub async fn add_peer(&self, request: AddPeerRequest) -> Result<AddPeerResponse, IndexerRestClientError> {
        self.send_post("network/connections", request).await
    }

    pub async fn get_substate(
        &self,
        id: &SubstateId,
        req: GetSubstateRequest,
    ) -> Result<GetSubstateResponse, IndexerRestClientError> {
        self.send_get(format!("substates/{id}"), req).await
    }

    pub async fn get_non_fungibles(
        &self,
        req: GetNonFungiblesRequest,
    ) -> Result<GetNonFungiblesResponse, IndexerRestClientError> {
        self.send_get("non-fungibles", req).await
    }

    pub async fn fetch_substates(
        &self,
        req: GetSubstatesRequest,
    ) -> Result<GetSubstatesResponse, IndexerRestClientError> {
        self.send_post("substates/fetch", req).await
    }

    pub async fn list_substates(
        &self,
        req: ListSubstatesRequest,
    ) -> Result<ListSubstatesResponse, IndexerRestClientError> {
        self.send_get("list_substates", req).await
    }

    pub async fn submit_transaction(
        &self,
        req: SubmitTransactionRequest,
    ) -> Result<SubmitTransactionResponse, IndexerRestClientError> {
        self.send_post("transactions", req).await
    }

    pub async fn submit_transaction_dry_run(
        &self,
        req: SubmitTransactionRequest,
    ) -> Result<SubmitTransactionDryRunResponse, IndexerRestClientError> {
        self.send_post("transactions/dry-run", req).await
    }

    pub async fn get_transaction_result(
        &self,
        req: GetTransactionResultRequest,
    ) -> Result<GetTransactionResultResponse, IndexerRestClientError> {
        self.send_get(format!("transactions/{}/result", req.transaction_id), ())
            .await
    }

    pub async fn list_recent_transactions(
        &self,
        req: ListRecentTransactionsRequest,
    ) -> Result<ListRecentTransactionsResponse, IndexerRestClientError> {
        self.send_get("transactions/recent", req).await
    }

    pub async fn list_cached_templates(
        &self,
        req: ListTemplatesRequest,
    ) -> Result<ListTemplatesResponse, IndexerRestClientError> {
        self.send_get("templates/cached", req).await
    }

    pub async fn get_template_definition(
        &self,
        template_address: TemplateAddress,
    ) -> Result<GetTemplateDefinitionResponse, IndexerRestClientError> {
        self.send_get(format!("templates/{template_address}"), ()).await
    }

    pub async fn get_epoch_manager_stats(&self) -> Result<GetEpochManagerStatsResponse, IndexerRestClientError> {
        self.send_get("epoch-manager/stats", ()).await
    }

    pub async fn stream_utxo_updates_protobuf(
        &self,
        req: GetUtxoUpdatesRequest,
    ) -> Result<ProtobufStream<protobuf::UtxoUpdatePayload>, IndexerRestClientError> {
        const PATH: &str = "utxos/stream";
        let url = format!("{}{}", self.endpoint, PATH);

        let resp = self
            .client
            .post(&url)
            .header(header::ACCEPT, "application/x-protobuf")
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(IndexerRestClientError::ErrorResponse {
                source: resp.error_for_status_ref().err().unwrap(),
                details: None,
            });
        }

        let stream = resp.bytes_stream();
        let stream = ProtobufStream::<protobuf::UtxoUpdatePayload>::new(stream);
        Ok(stream)
    }

    pub async fn get_utxos(&self, req: GetUtxosRequest) -> Result<GetUtxosResponse, IndexerRestClientError> {
        self.send_post("utxos/fetch", req).await
    }

    pub async fn list_utxos(&self, req: ListUtxosRequest) -> Result<ListUtxosResponse, IndexerRestClientError> {
        self.send_get("utxos", req).await
    }

    pub async fn list_transaction_receipts(
        &self,
        req: ListTransactionReceiptsRequest,
    ) -> Result<ListTransactionReceiptsResponse, IndexerRestClientError> {
        self.send_get("transaction-receipts", req).await
    }

    pub async fn get_transaction_receipt(
        &self,
        address: TransactionReceiptAddress,
    ) -> Result<GetTransactionReceiptResponse, IndexerRestClientError> {
        // We use as_object_key to get the string representation without the "txreceipt_" prefix
        self.send_get(format!("transaction-receipts/{}", address.as_object_key()), ())
            .await
    }

    pub async fn get_network_info(&self) -> Result<GetNetworkInfoResponse, IndexerRestClientError> {
        self.send_get("network", ()).await
    }

    pub async fn get_network_sync_state(&self) -> Result<GetNetworkSyncStateResponse, IndexerRestClientError> {
        self.send_get("network/stats", ()).await
    }

    pub async fn wait_until_ready(&self) -> Result<IndexerReadyResponse, IndexerRestClientError> {
        self.send_get("wait-until-ready", ()).await
    }

    pub async fn get_resource(&self, addr: ResourceAddress) -> Result<GetResourceResponse, IndexerRestClientError> {
        self.send_get(format!("resources/{addr}"), ()).await
    }

    pub async fn sse_events(&self) -> Result<SseEventStream, IndexerRestClientError> {
        let sse = self.send_sse("events", ()).await?;
        sse.into_stream()
    }

    async fn send_sse<P: Into<String>, T: Serialize>(
        &self,
        path: P,
        params: T,
    ) -> Result<SseEventStreamBuilder, IndexerRestClientError> {
        let path = path.into();

        // encode query params
        let query = serde_urlencoded::to_string(&params).map_err(|e| IndexerRestClientError::SerializeRequest {
            path: path.clone(),
            source: e.into(),
        })?;

        let mut url = format!("{}{}", self.endpoint, path);
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query);
        }

        let resp = self.client.get(url).send().await?;
        Ok(resp.into())
    }

    async fn send_get<P: Into<String>, T: Serialize, R: DeserializeOwned>(
        &self,
        path: P,
        params: T,
    ) -> Result<R, IndexerRestClientError> {
        let path = path.into();

        // encode query params
        let query = serde_urlencoded::to_string(&params).map_err(|e| IndexerRestClientError::SerializeRequest {
            path: path.clone(),
            source: e.into(),
        })?;

        let mut url = format!("{}{}", self.endpoint, path);
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query);
        }

        let resp = self.client.get(url).send().await?;
        handle_json_response(resp, path).await
    }

    async fn send_post<P: Into<String>, T: Serialize, R: DeserializeOwned>(
        &self,
        path: P,
        request: T,
    ) -> Result<R, IndexerRestClientError> {
        let path = path.into();

        let resp = self
            .client
            .post(format!("{}{}", self.endpoint, path))
            .json(&request)
            .send()
            .await?;

        handle_json_response(resp, path).await
    }
}

async fn handle_json_response<T: DeserializeOwned>(
    resp: reqwest::Response,
    path: String,
) -> Result<T, IndexerRestClientError> {
    if let Some(err) = resp.error_for_status_ref().err() {
        if let Ok(err_resp) = resp.json::<serde_json::Value>().await {
            return Err(IndexerRestClientError::ErrorResponse {
                source: err,
                details: err_resp.get("error").and_then(|v| v.as_str()).map(|s| s.to_string()),
            });
        }
        return Err(IndexerRestClientError::ErrorResponse {
            source: err,
            details: None,
        });
    }
    match resp.json().await {
        Ok(r) => Ok(r),
        Err(e) => Err(IndexerRestClientError::DeserializeResponse { path, source: e.into() }),
    }
}
