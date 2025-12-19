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

use std::net::SocketAddr;

use async_graphql::{
    http::{playground_source, GraphQLPlaygroundConfig},
    EmptyMutation,
    EmptySubscription,
    Schema,
};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    extract::Extension,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
    Json,
    Router,
};
use log::*;
use serde::Serialize;
use tari_ootle_app_utilities::tcp::try_bind_with_fallback;
use tower_http::cors::CorsLayer;

use crate::{
    graphql::model::events::{EventQuery, EventSchema},
    storage_sqlite::SqliteIndexerStore,
    substate_manager::SubstateManager,
    EventManager,
};

const LOG_TARGET: &str = "tari::indexer::graphql";

pub async fn run_graphql(
    preferred_address: SocketAddr,
    substate_manager: SubstateManager,
    store: SqliteIndexerStore,
) -> Result<(), anyhow::Error> {
    let event_manager = EventManager::new(store);
    let schema = Schema::build(EventQuery, EmptyMutation, EmptySubscription)
        .data(substate_manager)
        .data(event_manager)
        .finish();
    let router = Router::new()
        .route("/", get(graphql_playground).post(graphql_handler))
        .route("/health", get(health))
        .layer(CorsLayer::permissive())
        .layer(Extension(schema));

    let listener = try_bind_with_fallback(preferred_address).await?;
    let server = axum::serve(listener, router);
    let bind_addr = server.local_addr()?;
    info!(target: LOG_TARGET, "🌐 GraphQL listening on {bind_addr}");
    server.await?;

    Ok(())
}

#[derive(Serialize)]
struct Health {
    healthy: bool,
}

pub(crate) async fn health() -> impl IntoResponse {
    let health = Health { healthy: true };
    (StatusCode::OK, Json(health))
}

pub(crate) async fn graphql_playground() -> impl IntoResponse {
    Html(playground_source(
        GraphQLPlaygroundConfig::new("/").subscription_endpoint("/ws"),
    ))
}

pub(crate) async fn graphql_handler(Extension(schema): Extension<EventSchema>, req: GraphQLRequest) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}
