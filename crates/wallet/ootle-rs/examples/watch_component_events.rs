//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Example: Monitoring events emitted by a specific component via SSE.
//!
//! This example demonstrates how to:
//! - Subscribe to template-emitted events filtered by a component's substate ID
//! - Receive live events as they are finalized
//! - Resume from the last seen event after a disconnect using `after_id`
//!
//! ## Prerequisites
//! - A running localnet with an indexer
//! - A component that emits events (e.g. a token contract emitting Transfer events)

use std::str::FromStr;

use futures::StreamExt;
use ootle_rs::{
    Network,
    default_indexer_url,
    provider::{ProviderBuilder, TransactionEventFilter},
    template_types::ComponentAddress,
};
use tokio::pin;

// ---- Configuration ----

/// The component address to monitor for events.
/// Replace with a real component address from your environment.
const COMPONENT_ADDRESS: &str = "component_0000000000000000000000000000000000000000000000000000000000000000";

/// Optional: filter events by topic (e.g. "Transfer", "Mint", "Burn").
/// Set to None to receive all events from the component.
const TOPIC_FILTER: Option<&str> = None;

#[tokio::main]
async fn main() {
    env_logger::init();

    const NETWORK: Network = Network::LocalNet;
    let indexer_url = default_indexer_url(NETWORK);

    let provider = ProviderBuilder::new()
        .connect(indexer_url)
        .await
        .expect("Failed to connect to indexer");

    let component = ComponentAddress::from_str(COMPONENT_ADDRESS).expect("Invalid COMPONENT_ADDRESS");

    // Track the last seen event ID so we can resume after a disconnect.
    let mut last_event_id: Option<i64> = None;

    // Outer loop: reconnect on disconnect and resume from the last seen event.
    loop {
        println!("Subscribing to events for {component}...");
        if let Some(id) = last_event_id {
            println!("  Resuming from event id {id}");
        }

        let filter = TransactionEventFilter {
            topic: TOPIC_FILTER.map(String::from),
            substate_id: Some(component.into()),
            template_address: None,
            after_id: last_event_id,
        };

        let stream = provider.watch_events(filter).into_stream();
        pin!(stream);

        while let Some(result) = stream.next().await {
            match result {
                Ok(tx_event) => {
                    // Store the event ID for catch-up on reconnect.
                    last_event_id = Some(tx_event.id);

                    println!(
                        "[event id={}] tx={} topic={:?}",
                        tx_event.id,
                        tx_event.transaction_id,
                        tx_event.event.topic(),
                    );
                },
                Err(err) => {
                    eprintln!("Stream error: {err}");
                    break;
                },
            }
        }

        println!("Stream ended, reconnecting in 5s...");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
