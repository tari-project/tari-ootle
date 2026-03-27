# ootle_serde

[![Crates.io](https://img.shields.io/crates/v/ootle_serde.svg)](https://crates.io/crates/ootle_serde)
[![Documentation](https://docs.rs/ootle_serde/badge.svg)](https://docs.rs/ootle_serde)

`serde` helper modules for the Tari Ootle codebase. Each module is designed for
use with `#[serde(with = "…")]` and switches automatically between a
human-readable form (JSON) and a compact binary form (CBOR, MessagePack, etc.)
based on `Serializer::is_human_readable`.

## Modules

| Module              | Feature  | Human-readable                | Binary                           |
|---------------------|----------|-------------------------------|----------------------------------|
| `hex`               | `hex`    | hex string                    | raw bytes                        |
| `base64`            | `base64` | base64 string                 | raw bytes                        |
| `string`            | —        | `ToString` / `FromStr`        | native `Serialize`/`Deserialize` |
| `map`               | —        | array of `[key, value]` pairs | native map                       |
| `duration::seconds` | —        | integer seconds               | integer seconds                  |
| `cbor_value`        | `cbor`   | —                             | CBOR value helpers               |

## Example

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Record {
    #[serde(with = "ootle_serde::hex")]
    pub id: [u8; 32],

    #[serde(with = "ootle_serde::string")]
    pub amount: u64,

    #[serde(with = "ootle_serde::duration::seconds")]
    pub timeout: std::time::Duration,

    #[serde(with = "ootle_serde::map")]
    pub map: std::collections::HashMap<u64, String>,
}
```

## License

BSD-3-Clause. Copyright 2026 The Tari Project.
