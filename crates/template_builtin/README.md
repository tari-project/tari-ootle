# tari_template_builtin

Pre-compiled built-in templates for the Tari Ootle engine.

This crate provides well-known template addresses and, with the `templates` feature enabled, the pre-compiled WASM
bytecode for each built-in template.

## Built-in Templates

| Template       | Address          | Description                  |
|----------------|------------------|------------------------------|
| Account        | `0000...0000`    | Standard user account        |
| NFT Faucet     | `0000...0001`    | Mints NFTs for testing       |
| Faucet         | `0102030...0000` | Dispenses native TARI tokens |
| Liquidity Pool | `0000...0002`    | AMM liquidity pool           |

## Usage

```rust
use tari_template_builtin::{ACCOUNT_TEMPLATE_ADDRESS, is_builtin_template_address};

// Check if an address is a built-in template
assert!(is_builtin_template_address(&ACCOUNT_TEMPLATE_ADDRESS));
```

With the `templates` feature, access the compiled WASM bytecode:

```rust
use tari_template_builtin::get_template_builtin;

let wasm_bytes: & [u8] = get_template_builtin( & ACCOUNT_TEMPLATE_ADDRESS);
```

## Features

- **`templates`** — Includes the pre-compiled WASM bytecode for all built-in templates via `include_bytes!`. Without
  this feature, only the template addresses and `is_builtin_template_address` helper are available.

## Development

Template source lives in `templates/`. When building locally with the `templates` feature, `build.rs` compiles each
template to WASM and copies the output to `compiled/`. The compiled wasms are git-ignored but included in the
published crate so downstream consumers don't need the WASM toolchain.
