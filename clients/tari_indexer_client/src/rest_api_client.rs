//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use reqwest::{header, header::HeaderMap, IntoUrl, Url};
use serde::{de::DeserializeOwned, Serialize};
use tari_engine_types::substate::SubstateId;

use crate::{
    error::IndexerRestClientError,
    protobuf,
    protobuf_stream::ProtobufStream,
    types::{
        AddPeerRequest,
        AddPeerResponse,
        GetConnectionsResponse,
        GetEpochManagerStatsResponse,
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
        IndexerReadyResponse,
        ListRecentTransactionsRequest,
        ListRecentTransactionsResponse,
        ListSubstatesRequest,
        ListSubstatesResponse,
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

    pub async fn get_connections(&mut self) -> Result<GetConnectionsResponse, IndexerRestClientError> {
        self.send_get("network/connections", ()).await
    }

    pub async fn add_peer(&mut self, request: AddPeerRequest) -> Result<AddPeerResponse, IndexerRestClientError> {
        self.send_post("network/connections", request).await
    }

    pub async fn get_substate(
        &mut self,
        id: &SubstateId,
        version: Option<u32>,
        local_search_only: bool,
    ) -> Result<GetSubstateResponse, IndexerRestClientError> {
        self.send_get(format!("substates/{id}"), GetSubstateRequest {
            version,
            local_search_only,
        })
        .await
    }

    pub async fn get_non_fungibles(
        &mut self,
        req: GetNonFungiblesRequest,
    ) -> Result<GetNonFungiblesResponse, IndexerRestClientError> {
        self.send_get("non-fungibles", req).await
    }

    pub async fn fetch_substates(
        &mut self,
        req: GetSubstatesRequest,
    ) -> Result<GetSubstatesResponse, IndexerRestClientError> {
        self.send_post("substates/fetch", req).await
    }

    pub async fn list_substates(
        &mut self,
        req: ListSubstatesRequest,
    ) -> Result<ListSubstatesResponse, IndexerRestClientError> {
        self.send_get("list_substates", req).await
    }

    pub async fn submit_transaction(
        &mut self,
        req: SubmitTransactionRequest,
    ) -> Result<SubmitTransactionResponse, IndexerRestClientError> {
        self.send_post("transactions", req).await
    }

    pub async fn get_transaction_result(
        &mut self,
        req: GetTransactionResultRequest,
    ) -> Result<GetTransactionResultResponse, IndexerRestClientError> {
        self.send_get(format!("transactions/{}/result", req.transaction_id), ())
            .await
    }

    pub async fn list_recent_transactions(
        &mut self,
        req: ListRecentTransactionsRequest,
    ) -> Result<ListRecentTransactionsResponse, IndexerRestClientError> {
        self.send_get("transactions/recent", req).await
    }

    pub async fn get_template_definition(
        &mut self,
        req: GetTemplateDefinitionRequest,
    ) -> Result<GetTemplateDefinitionResponse, IndexerRestClientError> {
        self.send_get(format!("templates/{}", req.template_address), ()).await
    }

    // pub async fn get_non_fungibles(
    //     &mut self,
    //     req: GetNonFungiblesRequest,
    // ) -> Result<GetNonFungiblesResponse, IndexerRestClientError> {
    //     self.send_get("get_non_fungibles", req).await
    // }

    pub async fn get_epoch_manager_stats(&mut self) -> Result<GetEpochManagerStatsResponse, IndexerRestClientError> {
        self.send_get("epoch-manager/stats", ()).await
    }

    pub async fn stream_utxo_updates_protobuf(
        &mut self,
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

        let stream = resp.bytes_stream();
        let stream = ProtobufStream::<protobuf::UtxoUpdatePayload>::new(stream);
        Ok(stream)
    }

    pub async fn get_unspent_utxos(
        &mut self,
        req: GetUnspentUtxosRequest,
    ) -> Result<GetUnspentUtxosResponse, IndexerRestClientError> {
        self.send_post("utxos/fetch", req).await
    }

    pub async fn get_network_sync_state(&mut self) -> Result<GetNetworkSyncStateResponse, IndexerRestClientError> {
        self.send_get("network/stats", ()).await
    }

    pub async fn wait_until_ready(&mut self) -> Result<IndexerReadyResponse, IndexerRestClientError> {
        self.send_get("wait-until-ready", ()).await
    }

    async fn send_get<P: Into<String>, T: Serialize, R: DeserializeOwned>(
        &mut self,
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
        &mut self,
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
