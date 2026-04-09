# ootle.rs Examples

Runnable examples demonstrating how to use `ootle-rs` to interact with the Tari Ootle network.

## Prerequisites

All examples require a running **localnet** with an indexer (default: `http://127.0.0.1:12500`).

```bash
cargo run --example <example_name>
```

## Examples

### `fungible_transfer`

End-to-end public fungible token transfer: create a wallet, fund from faucet,
send multiple transfers in a single transaction, dry-run with fee estimation,
and verify balances.

### `stealth_transfer`

Confidential stealth transfers with input/output commitments, encrypted memos,
change handling, and stealth spending authorizers.

### `balance_query`

Query account balances and decrypt stealth UTXO values using ElGamal view keys.

Requires updating constants in the example: resource address, view secret key,
UTXO commitment, and max expected value.

> **Note:** UTXO decryption with on-the-fly lookup generation is slow.
> For production use, enable the `mmap-value-lookup` feature with pregenerated
> lookup files.

### `template_invoke`

Type-safe template invocation using the `ootle_template!` macro. Shows how to
define custom template interfaces, call template functions and component methods,
chain calls with workspace piping, and fall back to raw `TransactionBuilder`.

Requires updating template and component addresses in the example.

### `watch_component_events`

Real-time event monitoring via SSE (Server-Sent Events). Subscribe to events
filtered by component address and optional topic, with automatic reconnection
and resume-from-last-seen capability.

Requires updating the component address in the example.
