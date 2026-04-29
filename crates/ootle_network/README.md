# ootle-network

[![Crates.io](https://img.shields.io/crates/v/ootle-network.svg)](https://crates.io/crates/ootle-network)
[![Documentation](https://docs.rs/ootle-network/badge.svg)](https://docs.rs/ootle-network)

The `Network` enum identifying which Tari network a node, wallet, or transaction
belongs to (`MainNet`, `StageNet`, `NextNet`, `LocalNet`, `Igor`, `Esmeralda`).

This crate is intentionally minimal: it has no dependencies beyond `thiserror`
and is `no_std`-compatible so it can be used from template/WASM code as well as
from the validator node, wallet, indexer, and base-layer integration crates.

## Why a dedicated crate?

The byte values assigned to each variant (`MainNet = 0x00`, `Esmeralda = 0x26`,
…) must match the L1 (base layer) network enum exactly, since they are mixed
into address derivation, transaction hashing, and wire formats. Pulling the
definition into a small leaf crate keeps that single source of truth available
to every layer of the stack without dragging in heavier dependencies.

## Usage

```rust
use core::str::FromStr;
use ootle_network::Network;

let net = Network::from_str("esmeralda")?;
assert_eq!(net.as_byte(), 0x26);
assert_eq!(net.as_key_str(), "esmeralda");
assert!(net.is_testnet());

let parsed = Network::try_from(0x26u8)?;
assert_eq!(parsed, Network::Esmeralda);
# Ok::<(), ootle_network::NetworkParseError>(())
```

`FromStr` accepts the canonical lowercase name of each variant, plus `esme` as
a shorthand for `Esmeralda`. `Display` produces the same canonical name, so
round-tripping through strings is stable.

## Features

| Feature   | Default | Effect                                                                |
|-----------|---------|-----------------------------------------------------------------------|
| `std`     | yes     | Enables `std`. Disable for `no_std` builds (e.g. WASM templates).     |
| `serde`   | no      | Derives `Serialize`/`Deserialize` (lowercase variant names).          |
| `ts`      | no      | Derives `ts_rs::TS` for TypeScript binding generation.                |

## License

BSD-3-Clause. Copyright 2025 The Tari Project.
