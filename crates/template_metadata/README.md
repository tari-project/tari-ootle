# tari_ootle_template_metadata

Shared types for off-chain Tari Ootle template metadata with on-chain hash anchoring.

## Overview

Template authors describe their templates with structured metadata (name, version, description, tags, etc.) stored off-chain as CBOR. A domain-separated SHA-256 multihash of the CBOR is published on-chain alongside the template binary, allowing anyone to verify that off-chain metadata matches the on-chain record.

## Types

- **`TemplateMetadata`** — the metadata struct with fields from `Cargo.toml` (`[package]` and `[package.metadata.tari-template]`)
- **`MetadataHash`** — stack-allocated multihash (`Multihash<64>`) with no heap allocation. Serializes as hex string.
- **`MetadataHashWriter`** — an `std::io::Write` implementation that hashes data directly with domain-separated SHA-256. Used to compute the hash without intermediate buffer allocation.

## Usage

### Parsing metadata from Cargo.toml

```rust
use tari_ootle_template_metadata::from_cargo_toml;

let metadata = from_cargo_toml("path/to/Cargo.toml".as_ref())?;
println!("{}: {}", metadata.name, metadata.version);
```

### Computing the hash (zero-alloc)

```rust
use tari_ootle_template_metadata::TemplateMetadata;

let metadata = TemplateMetadata::new("my-template".into(), "1.0.0".into());
let hash = metadata.hash()?; // CBOR streams directly into the hasher
println!("Metadata hash: {hash}");
```

### Streaming CBOR to a file

```rust
use tari_ootle_template_metadata::TemplateMetadata;

let metadata = TemplateMetadata::new("my-template".into(), "1.0.0".into());
let mut file = std::fs::File::create("metadata.cbor")?;
metadata.write_cbor_to(&mut file)?;
```

### Reading CBOR from a file

```rust
use tari_ootle_template_metadata::TemplateMetadata;

let bytes = std::fs::read("metadata.cbor")?;
let metadata = TemplateMetadata::from_cbor(&bytes)?;
```

### Using MetadataHashWriter directly

```rust
use std::io::Write;
use tari_ootle_template_metadata::MetadataHashWriter;

let mut writer = MetadataHashWriter::new();
writer.write_all(b"some cbor bytes")?;
let hash = writer.finalize();
println!("Hash: {hash}");
```

## Cargo.toml field mapping

| TemplateMetadata field | Cargo.toml source |
|---|---|
| `schema_version` | Hardcoded to `1` |
| `name` | `[package] name` |
| `version` | `[package] version` |
| `description` | `[package] description` |
| `license` | `[package] license` |
| `repository` | `[package] repository` (parsed as `Url`) |
| `tags` | `[package.metadata.tari-template] tags` |
| `category` | `[package.metadata.tari-template] category` |
| `commit_hash` | `[package.metadata.tari-template] commit_hash` (full 40-char hex SHA-1 git object ID) |
| `documentation` | `[package.metadata.tari-template] documentation` (parsed as `Url`) |
| `homepage` | `[package.metadata.tari-template] homepage` (parsed as `Url`) |
| `logo_url` | `[package.metadata.tari-template] logo_url` (parsed as `Url`) |
| `supersedes` | `[package.metadata.tari-template] supersedes` (64-char hex template address) |
| `extra` | `[package.metadata.tari-template] extra` |

## Features

- **`json`** — enables `to_json()` / `from_json()` on `TemplateMetadata`
- **`borsh`** — enables `BorshSerialize` for `MetadataHash`
- **`ts`** — enables TypeScript type generation via `ts-rs`

## Hash details

The metadata hash is computed as:

```
SHA-256("com.tari.ootle.TemplateMetadata" || cbor_bytes)
```

Encoded as a [multihash](https://multiformats.io/multihash/) (code `0x12`, compatible with IPFS CIDv1).

## License

BSD-3-Clause
