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

use std::{future::IntoFuture, net::SocketAddr, sync::Arc};

use axum::{
    extract::{DefaultBodyLimit, Extension},
    routing::post,
    Router,
};
use axum_jrpc::{JrpcResult, JsonRpcExtractor};
use log::*;
use tari_ootle_app_utilities::tcp::try_bind_with_fallback;
use tower_http::cors::CorsLayer;

use super::handlers::JsonRpcHandlers;

const LOG_TARGET: &str = "tari::indexer::json_rpc";

pub async fn spawn_json_rpc(preferred_address: SocketAddr, handlers: JsonRpcHandlers) -> anyhow::Result<SocketAddr> {
    let router = Router::new()
        .route("/", post(handler))
        .route("/json_rpc", post(handler))
        .layer(Extension(Arc::new(handlers)))
        // Limit the body size to 5MB to allow for large transactions (wasm uploads)
        .layer(DefaultBodyLimit::max(5*1024*1024))
        .layer(CorsLayer::permissive());

    let listener = try_bind_with_fallback(preferred_address).await?;
    let server = axum::serve(listener, router);
    let listen_addr = server.local_addr()?;
    info!(target: LOG_TARGET, "🌐 JSON-RPC listening on {listen_addr}");
    tokio::spawn(server.into_future());

    Ok(listen_addr)
}

async fn handler(Extension(handlers): Extension<Arc<JsonRpcHandlers>>, value: JsonRpcExtractor) -> JrpcResult {
    info!(target: LOG_TARGET, "🌐 JSON-RPC request: {}", value.method);
    debug!(target: LOG_TARGET, "🌐 JSON-RPC body: {:?}", value);
    match value.method.as_str() {
        "rpc.discover" => handlers.rpc_discover(value),
        // P2p Network
        "get_identity" => handlers.get_identity(value).await,
        "add_peer" => handlers.add_peer(value).await,
        "get_comms_stats" => handlers.get_comms_stats(value).await,
        "get_connections" => handlers.get_connections(value).await,

        // Substates
        "list_substates" => handlers.list_substates(value).await,
        "get_substate" => handlers.get_substate(value).await,
        "get_substates" => handlers.get_substates(value).await,
        "inspect_substate" => handlers.inspect_substate(value).await,
        "get_non_fungibles" => handlers.get_non_fungibles(value).await,
        "get_utxo_updates" => handlers.get_utxo_updates(value).await,
        "get_unspent_utxos" => handlers.get_unspent_utxos(value).await,

        // Transactions
        "submit_transaction" => handlers.submit_transaction(value).await,
        "get_transaction_result" => handlers.get_transaction_result(value).await,

        // Templates
        "get_template_definition" => handlers.get_template_definition(value).await,
        "list_templates" => handlers.list_templates(value).await,
        "list_recent_transactions" => handlers.list_recent_transactions(value).await,

        // Misc
        "wait_until_ready" => handlers.wait_until_ready(value).await,
        "get_epoch_manager_stats" => handlers.get_epoch_manager_stats(value).await,
        "get_network_sync_state" => handlers.get_network_sync_state(value).await,
        method => Ok(value.method_not_found(method)),
    }
}
