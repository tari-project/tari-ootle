# tari_indexer_client

[![Crates.io](https://img.shields.io/crates/v/tari_indexer_client.svg)](https://crates.io/crates/tari_indexer_client)
[![Documentation](https://docs.rs/tari_indexer_client/badge.svg)](https://docs.rs/tari_indexer_client)

Client library for the Tari Ootle indexer node. Provides a typed async REST
client, a GraphQL client, Server-Sent Events (SSE) streaming, and a protobuf
streaming interface for consuming UTXO updates from a running indexer.

## Features

| Feature  | Default | Description                                                              |
|----------|---------|--------------------------------------------------------------------------|
| `client` | yes     | Enables the HTTP clients (`reqwest`-backed REST + SSE + protobuf stream) |
| `ts`     | no      | Generates TypeScript type bindings via `ts-rs`                           |
| `utoipa` | no      | Adds `utoipa` OpenAPI annotations to request/response types              |

## Key types

| Type                   | Description                                                      |
|------------------------|------------------------------------------------------------------|
| `IndexerRestApiClient` | Async REST client — the primary way to interact with the indexer |
| `SseEventStream`       | Server-Sent Events stream for live indexer events                |
| `ProtobufStream`       | Streaming protobuf reader for bulk UTXO update feeds             |

## Example

```rust
use tari_indexer_client::{connect_rest, types::{GetSubstateRequest, ListSubstatesRequest}};
use tari_engine_types::substate::SubstateId;
use futures::StreamExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = connect_rest("http://localhost:18300")?;

    // Fetch a specific substate by ID
    let response = client.get_substate(GetSubstateRequest {
        substate_id: some_substate_id,
        version: None,
    }).await?;

    println!("{:?}", response.substate);

    // Stream live events
    let mut events = client.sse_events().await?;

    while let Some(event) = events.next().await {
        println!("Received event: {:?}", event?);
    }

    Ok(())
}
```

## License

BSD-3-Clause. Copyright 2026 The Tari Project.
