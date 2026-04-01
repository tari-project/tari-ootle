# tari_ootle_template_build

Build-time metadata generation for Tari Ootle templates.

## Overview

This crate is added as a `[build-dependencies]` entry in a template crate. It reads metadata from `Cargo.toml`, generates a CBOR metadata file, and emits the metadata hash as cargo metadata for downstream tooling.

## Setup

Add to your template's `Cargo.toml`:

```toml
[build-dependencies]
tari_ootle_template_build = "0.2"
```

Create a `build.rs`:

```rust
fn main() {
    tari_ootle_template_build::TemplateMetadataBuilder::new()
        .build()
        .expect("Failed to generate template metadata");
}
```

## Builder options

The builder reads metadata from `Cargo.toml` by default. Any field can be overridden programmatically:

```rust
fn main() {
    tari_ootle_template_build::TemplateMetadataBuilder::new()
        .description("My DeFi template")
        .tags(vec!["defi", "token", "swap"])
        .category("defi")
        .homepage("https://example.com")
        .extra_entry("audit", "https://example.com/audit-report")
        .enable_json_output() // also generate a JSON file
        .build()
        .expect("Failed to generate template metadata");
}
```

### Available overrides

| Method | Description |
|---|---|
| `.description(...)` | Override description from Cargo.toml |
| `.tags(...)` | Override tags |
| `.category(...)` | Override category |
| `.repository(...)` | Override repository URL |
| `.documentation(...)` | Override documentation URL |
| `.homepage(...)` | Override homepage URL |
| `.license(...)` | Override license |
| `.extra(map)` | Replace entire extra metadata map |
| `.extra_entry(k, v)` | Add a single extra key-value pair |
| `.enable_json_output()` | Also write `template_metadata.json` |

## Output

On a successful `build()`:

- Writes `template_metadata.cbor` to `OUT_DIR`
- Optionally writes `template_metadata.json` to `OUT_DIR`
- Emits `cargo::metadata=TEMPLATE_METADATA_HASH=<hex>` for downstream tools
- Returns `TemplateBuildOutput` with the hash, file paths, and resolved metadata

## Cargo.toml metadata

The builder reads from standard `[package]` fields and `[package.metadata.tari-template]`:

```toml
[package]
name = "fungible-token"
version = "1.2.0"
description = "A standard fungible token"
license = "BSD-3-Clause"
repository = "https://github.com/example/fungible-token"

[package.metadata.tari-template]
tags = ["token", "fungible", "defi"]
category = "token"
documentation = "https://docs.example.com/fungible-token"
homepage = "https://example.com"

[package.metadata.tari-template.extra]
audit = "https://example.com/audit-report"
```

## License

BSD-3-Clause
