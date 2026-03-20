# Tari Ootle - Claude Code Cheatsheet

## Project Structure

- The project is called "Ootle" or "Tari Ootle" (not "DAN" or "Tari DAN")
- **Crates** are in `crates/`, applications in `applications/`, clients in `clients/`
- Wallet daemon package: `tari_ootle_walletd` (in `applications/tari_walletd/`)
- Wallet CLI: `tari_wallet_cli` (in `applications/tari_wallet_cli/`)
- Transaction manifest parser: `tari_transaction_manifest` (in `crates/transaction_manifest/`)
- Transaction types/instructions: `tari_ootle_transaction` (in `crates/transaction/`)
- Template lib types (OwnerRule, RistrettoPublicKeyBytes, etc.): `tari_template_lib_types` (in
  `crates/template_lib_types/`)
- CBOR serialization: `tari_bor` (in `crates/tari_bor/`) - wraps ciborium, re-exports serde

## Key Facts

### Token Amounts

- All amounts in code/manifests are in **microtari**: 1 TARI = 1,000,000 microtari
- In manifests, use `TARI` as the resource identifier (not `XTR`, which is deprecated)
- User-facing: use **tTARI** (testnet) or **$TARI** (mainnet)

### Transaction Manifests

- DSL parsed by `tari_transaction_manifest::parse_manifest()`
- Parser is in `crates/transaction_manifest/src/parser.rs`, generator in `generator.rs`
- Macros: `var!`/`arg!`/`global!`, `new_component_addr!`, `new_resource_addr!`, `create_account!`, `drop_all_proofs!`,
  log macros
- `create_account!(owner_pk)` generates `Instruction::CreateAccount` - idempotent account creation
- Variables passed to manifests are parsed via `ManifestValue::FromStr` which handles: substate IDs (`component_<hex>`,
  `resource_<hex>`, etc.), NFT IDs, Rust literals, and raw hex bytes (for public keys)

### Wallet Daemon JSON-RPC

- Default port: 5100, endpoint: `/json_rpc`, ask the user if the daemon is running on a different port
- Auth: check `auth.method`, then `auth.request` with `credentials: "None"` for no-auth setups
- JWT tokens expire after 5 minutes
- Key endpoints: `accounts.get_default`, `accounts.list`, `transactions.submit_manifest`, `transactions.wait_result`
- `transactions.submit_manifest` variables are strings parsed by `ManifestValue::FromStr`

### PR Workflow

- Base branch for PRs is `development`, not `main`

### Building

- Standard cargo workspace. Use `-p <package_name>` to build/test specific crates
