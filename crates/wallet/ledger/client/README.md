# `ootle_ledger_client`

## Overview

`ootle_ledger_client` is the host-side client for the Tari Ootle Ledger app. `LedgerClient` wraps
any [`ledger_transport::Exchange`](https://docs.rs/ledger-transport) APDU transport and exposes
the app's instruction set:

- app name and version queries,
- on-device key derivation (`get_public_key`) — secrets never leave the device,
- streamed transaction signing (`sign_transaction`) for both authorization ("add signer") and
  seal signatures, with optional stealth key derivation for confidential transfers.

The wire format is defined in
[`ootle_ledger_common`](https://crates.io/crates/ootle_ledger_common), which the device app also
depends on, so host and device share a single protocol definition. Higher-level signers (e.g.
`ootle-rs`'s ledger signer) build on this crate.

## Transports

Transports are feature-gated; enable the one you need:

| Feature               | Provides                                                          |
|-----------------------|-------------------------------------------------------------------|
| `hid-transport`       | `LedgerHidClient` — native USB HID for physical devices           |
| `speculos-transport`  | `SpeculosTransport` — drives the [Speculos] emulator's REST API   |

Any other `Exchange` implementation (e.g. TCP, Bluetooth) works too — pass it to
`LedgerClient::new`.

[Speculos]: https://github.com/LedgerHQ/speculos

## Usage

```rust,no_run
use ootle_ledger_client::{LedgerClient, speculos_transport::SpeculosTransport};
use ootle_ledger_common::arg_types::KeyType;

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
// Any `Exchange` transport works; Speculos shown here.
let client = LedgerClient::new(SpeculosTransport::new());

let version = client.get_app_version().await?;
let public_key = client.get_public_key(0, 0, KeyType::Account).await?;
# Ok(())
# }
```

Signing streams the canonical transaction preimage fields to the device, which recomputes the
signing message and Schnorr challenge itself before showing the user review — see
`LedgerClient::sign_transaction` and the protocol description in `ootle_ledger_common`.

## Testing

- `tests/signing_recipe.rs` is a host reference implementation of the on-device signing recipe,
  proving it produces signatures that verify under `tari_ootle_transaction` / `tari_crypto` —
  no device required.
- The `#[ignore]`d integration tests exercise the real exchange against Speculos running the
  Ootle app:

  ```sh
  cargo test -p ootle_ledger_client --features speculos-transport -- --ignored
  ```

  The Speculos URL defaults to `http://localhost:5000` and can be overridden with `SPECULOS_URL`.

## Documentation

Detailed documentation is available at
[docs.rs/ootle_ledger_client](https://docs.rs/ootle_ledger_client).
