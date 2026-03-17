# ootle-wasm-core

Pure Rust library implementing the crypto and encoding operations needed by Tari Ootle WASM clients. This crate has no WASM dependencies and can be used natively from any Rust project.

## Modules

### `bor` — BOR (CBOR) encoding

Encodes `Transaction` values using `tari_bor` and returns base64 strings matching the `TransactionEnvelope` format used by the Ootle network.

- `bor_encode_transaction(&Transaction) → Result<String>`
- `bor_encode_transaction_json(json: &str) → Result<String>`

### `hash` — Transaction hashing

Produces the 64-byte signing message for an `UnsignedTransactionV1`. Delegates to `TransactionSignature::create_message_v1` to guarantee byte-identical output with the rest of the Tari codebase.

- `hash_unsigned_transaction(&UnsignedTransactionV1, seal_signer_hex: &str) → Result<Vec<u8>>`
- `hash_unsigned_transaction_json(json: &str, seal_signer_hex: &str) → Result<Vec<u8>>`

### `sign` — Schnorr signing and key management

Ristretto255 Schnorr signatures and keypair operations.

- `schnorr_sign(secret_key_hex: &str, message: &[u8]) → Result<SchnorrSignatureResult>`
- `public_key_from_secret_key(secret_key_hex: &str) → Result<String>`
- `generate_keypair() → KeypairResult`

### `error` — Error types

`OotleWasmError` covers JSON, BOR, hex, and key-related failures.

## Testing

```bash
cargo test -p ootle-wasm-core
```

Tests verify that hashing output is identical to `tari_ootle_transaction`, that JSON round-trips produce the same hashes, and that keypair generation and signing work correctly.
