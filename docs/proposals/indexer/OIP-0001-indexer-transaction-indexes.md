# OIP-0001: Indexing Transaction Substate Data

```
OIP Number: 0001
Title: Indexing transaction substate data
Status: Draft
Author(s): Stanley Bondi
Created: 2026-02-25
```

## Abstract

This proposal extends the Tari Ootle indexer to track which substates are created or
consumed by each transaction and to expose this information via a new Server-Sent
Events (SSE) stream at `/transactions/stream`. The stream may be filtered by one or
more `SubstateId`s, enabling wallets to be notified of any transaction that touched a
substate they care about (e.g. a vault, an account component). The Substate-to-transaction
link is derived entirely from existing `TransactionReceipt` data obtained during the
indexer's routine state-sync with validator nodes.

The `TransactionFinalizedEvent` type is extended to carry the list of affected
substates.

## Motivation

There is currently no scalable way (apart from constantly fetching all new transactions)
for indexer clients (wallets etc.) to know about transactions that have affected substates
that it cares about. For example, Alice receives funds from Bob, but Alice has no way to
know which transaction deposited those funds.

This creates a confusing experience: balances change with no corresponding entry in the
transaction log.

Exchanges and custodians currently have no reliable way to detect
transactions that touch multiple accounts/vaults.

This problem can be solved by creating a mapping from `SubstateId` to
`transaction_id` and allowing clients to subscribe to a stream of transactions
that affected specific (or any) substates.

## Specification

### Overview

1. The `substate_transitions` database table is extended with a nullable
   `transaction_id` column so that each transition can be associated with the
   transaction that caused it.
2. The `TransactionFinalizedEvent` (broadcast on the existing `/events` SSE stream and
   on the new `/transactions/stream` SSE stream) is extended to include the list of
   substates affected by the transaction and the shard state version at which the
   transition was finalised.
3. A new SSE endpoint `/transactions/stream` is added. It accepts optional
   `filter_by_substate_id` query parameters to restrict the stream to transactions that touched
   specific substates.
4. The wallet SDK and `ootle-rs` libraries are updated to make use of the new
   stream.

### Technical Details

#### 3.1 Database Schema Change

Add a `transaction_id` column to the existing `substate_transitions` table and an
index to support efficient substate-to-transaction lookups:

```sql
ALTER TABLE substate_transitions
    ADD COLUMN transaction_id TEXT;

CREATE INDEX idx_substate_transitions_substate_id
    ON substate_transitions (substate_id);
```

The column is nullable to preserve backwards compatibility with rows written before
this change was deployed.

#### 3.2 Updated `TransactionFinalizedEvent`

The event type in `clients/tari_indexer_client/src/event.rs` is extended:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionFinalizedEvent {
    pub transaction_id: TransactionId,
    pub outcome: FinalizeOutcome,
    /// Substates that were created (is_up = true) or consumed (is_up = false)
    /// by this transaction.
    pub substates: Vec<SubstateTransitionSummary>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SubstateTransitionSummary {
    pub substate_id: SubstateId,
    /// The version of the substate after this transition.
    pub version: u32,
    /// The shard-level state version at which this transition was finalised.
    pub state_version: StateVersion,
    /// `true` if this substate was created/updated; `false` if consumed.
    pub is_up: bool,
}
```

The `substates` list is populated in the state-sync worker
(`applications/tari_indexer/src/network_state_sync/worker.rs`) when a
`TransactionReceipt` substate is processed:

- **Upped substates** are sourced from `TransactionReceipt::diff_summary.upped`
  (`DiffSummary`, which already carries `substate_id`, `version`, and `value_hash`).
  The shard `state_version` is available from the surrounding shard-sync context at
  the point the receipt is processed.
- **Downed substates** can be derived from the inputs of the transaction body that is
  already stored in the `transactions` table, or inferred by looking up which existing
  `substate_transitions` rows share the same `substate_id` with a lower version.

No changes to `TransactionReceipt` or any validator-node P2P messages are required.

#### 3.3 New SSE Endpoint: `GET /transactions/stream`

The endpoint follows the same pattern as the existing `/events` SSE endpoint.

**Route**

```
GET /transactions/stream
```

**Query Parameters**

| Parameter               | Type                          | Required | Description                                                                     |
|-------------------------|-------------------------------|----------|---------------------------------------------------------------------------------|
| `filter_by_substate_id` | `SubstateId[]` (string, list) | No       | One or more substate IDs. When absent, all finalised transactions are streamed. |

Example request:

```
GET /transactions/stream?filter_by_substate_id=component_1234...&filter_by_substate_id=vault_abcd...
```

**Response**

`Content-Type: text/event-stream`

Each event has:

```
event: TransactionFinalized
data: <JSON-encoded TransactionFinalizedEvent>
```

**Filtering Logic**

When one or more `filter_by_substate_id` values are supplied, an event is only emitted to that
client if the transaction's `substates` list contains at least one matching
`SubstateId`. Matching is performed in the SSE handler after the broadcast is received,
keeping the core sync worker decoupled from per-client filter state.

**Handler Pseudocode**

```rust
async fn sse_transactions(
    Query(params): Query<TransactionStreamParams>,
    State(ctx): State<HandlerContext>,
) -> Sse<impl Stream<Item=Result<Event, Infallible>>> {
    let mut rx = ctx.subscribe_events();
    let filter_ids: HashSet<SubstateId> = params.filter_by_substate_id.into_iter().collect();

    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(IndexerEvent::TransactionFinalized(ev)) => {
                    if filter_ids.is_empty()
                        || ev.substates.iter().any(|s| filter_ids.contains(&s.substate_id))
                    {
                        let data = serde_json::to_string(&ev).unwrap();
                        yield Ok(Event::default().event("TransactionFinalized").data(data));
                    }
                }
                _ => {}
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

#### 3.4 New Request/Response Types

```rust
// clients/tari_indexer_client/src/types.rs

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct TransactionStreamParams {
    #[serde(default)]
    pub filter_by_substate_id: Vec<SubstateId>,
}
```

## Rationale

### Why extend `TransactionFinalizedEvent` rather than adding a separate event type?

The `TransactionFinalizedEvent` is already consumed by clients that subscribe to
`/events`. Enriching it avoids breaking those clients unnecessarily (the new field is
additive) and reduces the number of distinct event types consumers must handle.

### Why SSE rather than WebSockets?

The existing indexer streaming infrastructure uses SSE (the `/events` endpoint) and
unidirectional protobuf streams (the `/utxos/stream` endpoint). SSE is sufficient for
this use case (server-to-client push of transaction events) and is simpler to implement
and operate than WebSockets.

### Why filter by `SubstateId` rather than by account address or template?

Filtering by `SubstateId` is the most flexible primitive: an account component, a
vault, or any other substate can be used as a filter. Higher-level filters (e.g.
"all substates belonging to account X") can be composed in the client by first
resolving the relevant `SubstateId`s and then subscribing with that list.

## Backwards Compatibility

### `TransactionFinalizedEvent`

Adding `substates: Vec<SubstateTransitionSummary>` is a backwards-compatible JSON
change: existing consumers that do not recognise the field will ignore it. However,
clients that pattern-match exhaustively on the struct in Rust will need to be updated.
The field should be tagged `#[serde(default)]` so that events stored or forwarded by
older indexer versions deserialise without error.

### `substate_transitions` table

The new `transaction_id` column is nullable. The Diesel migration adds the column with
`DEFAULT NULL`, which is compatible with existing rows. Read queries that do not
reference the column are unaffected. The indexer may also resync historical data.

### No P2P protocol changes

No validator-node RPC methods are added or removed, so validator nodes and indexers at
different versions remain interoperable.

## Test Cases

- **Unit**: `TransactionFinalizedEvent` serialises and deserialises correctly with and
  without the `substates` field (backwards compat).
- **Integration – unfiltered stream**: Subscribing to `/transactions/stream` with no
  filter parameters yields an event for every finalised transaction.
- **Integration – filtered stream**: Subscribing with a specific `filter_by_substate_id` yields
  only events for transactions that touched that substate; unrelated transactions do
  not appear.
- **Integration – multiple filters**: Subscribing with two `filter_by_substate_id` values yields
  events for transactions that touched *either* substate.
- **Integration – substate_transitions linkage**: After a transaction is finalised, the
  `substate_transitions` rows for the affected substates carry the correct
  `transaction_id`.
- **Wallet SDK**: The `subscribe_transactions` method surfaces inbound-transaction
  events to the wallet when one of its managed vaults is credited.

## Implementation

1. Add Diesel migration to extend `substate_transitions` with `transaction_id` and
   create the two new indexes.
2. Update the state-sync worker to populate `transaction_id` on `substate_transitions`
   rows when processing a `TransactionReceipt`.
3. Extend `TransactionFinalizedEvent` with the `substates` field and populate it in
   the worker.
4. Add `GET /transactions/stream` route, handler, and `TransactionStreamParams` type.
5. Add `TransactionStreamParams` and `SubstateTransitionSummary` to the indexer client
   crate.
6. Add `subscribe_transactions` to `WalletNetworkInterface` and implement it in
   `IndexerRestApiNetworkInterface`.
7. Add `subscribeTransactions` helper to the `ootle-rs` TypeScript indexer client.
8. Write integration tests covering the filtering scenarios described above.

## References

- Existing SSE handler:
  `applications/tari_indexer/src/rest_api/handlers/events.rs`
- Existing event types:
  `clients/tari_indexer_client/src/event.rs`
- `TransactionReceipt` and `DiffSummary`:
  `crates/engine_types/src/transaction_receipt.rs`
- State-sync worker (where `TransactionFinalizedEvent` is emitted):
  `applications/tari_indexer/src/network_state_sync/worker.rs`
- Database schema:
  `applications/tari_indexer/src/storage_sqlite/schema.rs`
- Wallet network interface:
  `crates/wallet/sdk_services/src/indexer_rest_api.rs`

## Copyright

This document is released under the BSD 3-Clause License.
