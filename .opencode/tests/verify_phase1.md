# Verify Phase 1: Account Balance Changes Data Layer

## Context

This is Phase 1 of the balance change log feature (#1949). The data layer has been implemented but cannot be built/tested on Windows due to the OpenSSL transitive dependency via `webauthn-rs-core`. This needs verification on a Linux environment.

## Task

Run the following on a Linux machine with Rust installed and the repo checked out at commit `0e3c4290c`:

### 1. Build SDK crate

```bash
cargo build -p tari_ootle_wallet_sdk 2>&1
```

If compilation errors occur, fix them and commit the fixes back to the `development` branch of `sanrishi/tari-ootle`.

### 2. Build SQLite storage crate

```bash
cargo build -p tari_ootle_wallet_storage_sqlite 2>&1
```

### 3. Run storage tests

```bash
cargo test -p tari_ootle_wallet_storage_sqlite --test balance_changes 2>&1
```

All 6 tests should pass:
- `tx_driven_change`
- `multi_vault_transaction`
- `non_transaction_recovery_change`
- `idempotent_rescan`
- `pagination`
- `zero_balance_change_not_logged`

### 4. Run full workspace build (optional but recommended)

```bash
cargo build --workspace 2>&1
```

### 5. If fixes needed

If any file needs a fix, edit it, commit, and push:
```bash
git add <files>
git commit -m "fix: Phase 1 compilation fixes"
git push fork development
```

## Files to check if errors occur

| File | Likely issues |
|---|---|
| `crates/wallet/sdk/src/models/balance_change.rs` | Import paths, `PrimitiveDateTime` usage |
| `crates/wallet/storage_sqlite/src/models/balance_change.rs` | `TransactionId::from_hex` signature |
| `crates/wallet/storage_sqlite/src/reader.rs` | Diesel join syntax, `into_boxed()` chaining |
| `crates/wallet/storage_sqlite/src/writer.rs` | Closure borrowing, match ergonomics |
| `crates/wallet/storage_sqlite/tests/balance_changes.rs` | `TransactionId::new([u8; 32])` construction |

## Report back

After completion, report:
- Whether all tests passed
- Any errors encountered and how they were fixed
- The final commit hash on `sanrishi/tari-ootle`
