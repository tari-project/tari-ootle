//   Copyright 2022. The Tari Project
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
mod error;
pub use error::ValidatorNodeClientError;

pub mod types;

use reqwest::{IntoUrl, Url, header, header::HeaderMap};
use serde::{Serialize, de::DeserializeOwned};
use serde_json as json;
use serde_json::json;

use crate::types::*;

#[derive(Debug, Clone)]
pub struct ValidatorNodeClient {
    client: reqwest::Client,
    endpoint: Url,
    request_id: i64,
}

impl ValidatorNodeClient {
    pub fn connect<T: IntoUrl>(endpoint: T) -> Result<Self, ValidatorNodeClientError> {
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
            request_id: 0,
        })
    }

    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    pub async fn get_identity(&mut self) -> Result<GetIdentityResponse, ValidatorNodeClientError> {
        self.send_request("get_identity", json!({})).await
    }

    pub async fn get_epoch_manager_stats(&mut self) -> Result<GetEpochManagerStatsResponse, ValidatorNodeClientError> {
        self.send_request("get_epoch_manager_stats", json!({})).await
    }

    pub async fn get_consensus_status(&mut self) -> Result<GetConsensusStatusResponse, ValidatorNodeClientError> {
        self.send_request("get_consensus_status", json!({})).await
    }

    pub async fn get_diagnostic_events(
        &mut self,
        request: GetDiagnosticEventsRequest,
    ) -> Result<GetDiagnosticEventsResponse, ValidatorNodeClientError> {
        self.send_request("get_diagnostic_events", request).await
    }

    pub async fn clear_diagnostic_events(
        &mut self,
        request: ClearDiagnosticEventsRequest,
    ) -> Result<ClearDiagnosticEventsResponse, ValidatorNodeClientError> {
        self.send_request("clear_diagnostic_events", request).await
    }

    pub async fn create_backup_snapshot(&mut self) -> Result<CreateBackupSnapshotResponse, ValidatorNodeClientError> {
        self.send_request("create_backup_snapshot", json!({})).await
    }

    pub async fn get_state(&mut self, request: GetStateRequest) -> Result<GetStateResponse, ValidatorNodeClientError> {
        self.send_request("get_state", request).await
    }

    pub async fn get_substate(
        &mut self,
        request: GetSubstateRequest,
    ) -> Result<GetSubstateResponse, ValidatorNodeClientError> {
        self.send_request("get_substate", request).await
    }

    pub async fn get_template(
        &mut self,
        request: GetTemplateRequest,
    ) -> Result<GetTemplateResponse, ValidatorNodeClientError> {
        self.send_request("get_template", request).await
    }

    pub async fn get_transaction(
        &mut self,
        request: GetTransactionRequest,
    ) -> Result<GetTransactionResponse, ValidatorNodeClientError> {
        self.send_request("get_transaction", request).await
    }

    pub async fn get_transaction_result(
        &mut self,
        request: GetTransactionResultRequest,
    ) -> Result<GetTransactionResultResponse, ValidatorNodeClientError> {
        self.send_request("get_transaction_result", request).await
    }

    pub async fn list_blocks(
        &mut self,
        request: ListBlocksRequest,
    ) -> Result<ListBlocksResponse, ValidatorNodeClientError> {
        self.send_request("list_blocks", request).await
    }

    pub async fn list_blocks_paginated(
        &mut self,
        request: GetBlocksRequest,
    ) -> Result<GetBlocksResponse, ValidatorNodeClientError> {
        self.send_request("get_blocks", request).await
    }

    pub async fn submit_transaction(
        &mut self,
        request: SubmitTransactionRequest,
    ) -> Result<SubmitTransactionResponse, ValidatorNodeClientError> {
        self.send_request("submit_transaction", request).await
    }

    pub async fn add_peer(&mut self, request: AddPeerRequest) -> Result<AddPeerResponse, ValidatorNodeClientError> {
        self.send_request("add_peer", request).await
    }

    pub async fn get_block(&mut self, request: GetBlockRequest) -> Result<GetBlockResponse, ValidatorNodeClientError> {
        self.send_request("get_block", request).await
    }

    pub async fn prepare_layer_one_transaction(
        &mut self,
        request: PrepareLayerOneTransactionRequest,
    ) -> Result<PrepareLayerOneTransactionResponse, ValidatorNodeClientError> {
        self.send_request("prepare_layer_one_transaction", request).await
    }

    pub async fn get_tx_pool(&mut self) -> Result<GetTxPoolResponse, ValidatorNodeClientError> {
        self.send_request("get_tx_pool", json!({})).await
    }

    pub async fn get_filtered_blocks_count(
        &mut self,
        request: GetFilteredBlocksCountRequest,
    ) -> Result<GetBlocksCountResponse, ValidatorNodeClientError> {
        self.send_request("get_filtered_blocks_count", request).await
    }

    pub async fn get_mempool_stats(&mut self) -> Result<GetMempoolStatsResponse, ValidatorNodeClientError> {
        self.send_request("get_mempool_stats", json!({})).await
    }

    pub async fn get_shard_key(
        &mut self,
        request: GetShardKeyRequest,
    ) -> Result<GetShardKeyResponse, ValidatorNodeClientError> {
        self.send_request("get_shard_key", request).await
    }

    pub async fn get_all_vns(
        &mut self,
        request: GetAllVnsRequest,
    ) -> Result<GetAllVnsResponse, ValidatorNodeClientError> {
        self.send_request("get_all_vns", request).await
    }

    pub async fn get_comms_stats(&mut self) -> Result<GetCommsStatsResponse, ValidatorNodeClientError> {
        self.send_request("get_comms_stats", json!({})).await
    }

    pub async fn get_connections(&mut self) -> Result<GetConnectionsResponse, ValidatorNodeClientError> {
        self.send_request("get_connections", json!({})).await
    }

    fn next_request_id(&mut self) -> i64 {
        self.request_id += 1;
        self.request_id
    }

    async fn send_request<T: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &str,
        params: T,
    ) -> Result<R, ValidatorNodeClientError> {
        let params = json::to_value(params).map_err(|e| ValidatorNodeClientError::SerializeRequest {
            source: e,
            method: method.to_string(),
        })?;
        let request_json = json!(
            {
                "jsonrpc": "2.0",
                "id": self.next_request_id(),
                "method": method,
                "params": params,
            }
        );
        let resp = self
            .client
            .post(self.endpoint.clone())
            .body(request_json.to_string())
            .send()
            .await?
            .error_for_status()?;
        let val = resp.json().await?;
        let resp = jsonrpc_result(val)?;
        // Response might not deserialize to R....
        match serde_json::from_value(resp) {
            Ok(r) => Ok(r),
            Err(e) => Err(ValidatorNodeClientError::DeserializeResponse {
                source: e,
                method: method.to_string(),
            }),
        }
    }
}

fn jsonrpc_result(mut val: json::Value) -> Result<json::Value, ValidatorNodeClientError> {
    if let Some(err) = val.get("error") {
        let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        let message = err.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error");
        return Err(ValidatorNodeClientError::RequestFailedWithStatus {
            code,
            message: message.to_string(),
        });
    }

    let result =
        val.get_mut("result")
            .map(json::Value::take)
            .ok_or_else(|| ValidatorNodeClientError::InvalidResponse {
                message: "Missing result field".to_string(),
            })?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use tari_ootle_common_types::optional::IsNotFoundError;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;

    /// Serves a single HTTP response with the given status line and body, after reading the full request.
    async fn serve_once(status: &'static str, content_type: &'static str, body: &'static str) -> Url {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).await.unwrap();
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some(header_end) = text.find("\r\n\r\n") {
                    let content_length = text[..header_end]
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            if name.eq_ignore_ascii_case("content-length") {
                                value.trim().parse::<usize>().ok()
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + content_length {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: \
                 close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        });
        Url::parse(&format!("http://{addr}/json_rpc")).unwrap()
    }

    #[tokio::test]
    async fn non_2xx_response_fails_with_http_status() {
        let url = serve_once("502 Bad Gateway", "text/html", "<html>Bad Gateway</html>").await;
        let mut client = ValidatorNodeClient::connect(url).unwrap();
        let err = client.get_mempool_stats().await.unwrap_err();
        assert!(
            matches!(
                err,
                ValidatorNodeClientError::RequestFailed { ref source }
                    if source.status() == Some(reqwest::StatusCode::BAD_GATEWAY)
            ),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn http_404_is_a_routing_failure_and_not_a_missing_resource() {
        let url = serve_once("404 Not Found", "text/plain", "404 page not found").await;
        let mut client = ValidatorNodeClient::connect(url).unwrap();
        let err = client.get_mempool_stats().await.unwrap_err();
        assert!(
            matches!(
                err,
                ValidatorNodeClientError::RequestFailed { ref source }
                    if source.status() == Some(reqwest::StatusCode::NOT_FOUND)
            ),
            "unexpected error: {err}"
        );
        assert!(!err.is_not_found_error());
    }

    #[tokio::test]
    async fn jsonrpc_not_found_error_is_a_not_found_error() {
        let url = serve_once(
            "200 OK",
            "application/json",
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":404,"message":"Substate not found"}}"#,
        )
        .await;
        let mut client = ValidatorNodeClient::connect(url).unwrap();
        let err = client.get_mempool_stats().await.unwrap_err();
        assert!(err.is_not_found_error(), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn jsonrpc_success_deserializes_result() {
        let url = serve_once(
            "200 OK",
            "application/json",
            r#"{"jsonrpc":"2.0","id":1,"result":{"size":42}}"#,
        )
        .await;
        let mut client = ValidatorNodeClient::connect(url).unwrap();
        let stats = client.get_mempool_stats().await.unwrap();
        assert_eq!(stats.size, 42);
    }

    #[test]
    fn jsonrpc_result_extracts_error_code_and_message() {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": 404,
                "message": "Substate not found"
            }
        });
        let err = jsonrpc_result(payload).unwrap_err();
        assert!(err.is_not_found_error());
        assert!(matches!(
            err,
            ValidatorNodeClientError::RequestFailedWithStatus {
                code: 404,
                ref message
            } if message == "Substate not found"
        ));
    }

    #[test]
    fn jsonrpc_result_rejects_missing_result_field() {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1
        });
        let err = jsonrpc_result(payload).unwrap_err();
        assert!(matches!(err, ValidatorNodeClientError::InvalidResponse { .. }));
    }
}
