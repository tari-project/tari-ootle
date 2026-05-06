# tari_ootle_walletd_client

[![crates.io](https://img.shields.io/crates/v/tari_ootle_walletd_client)](https://crates.io/crates/tari_ootle_walletd_client)
[![docs.rs](https://docs.rs/tari_ootle_walletd_client/badge.svg)](https://docs.rs/tari_ootle_walletd_client)

A JSON-RPC 2.0 client for the Tari wallet daemon (`tari_walletd`). Provides strongly-typed async methods for account
management, transactions, stealth/confidential transfers, NFTs, and more.

## Usage

```rust,ignore
use tari_ootle_walletd_client::WalletDaemonClient;
use tari_ootle_walletd_client::types::*;

// Connect to a running wallet daemon
let mut client = WalletDaemonClient::connect("http://localhost:18000", None)?;

// Authenticate and set the JWT token
let auth_resp = client.auth_request(AuthLoginRequest {
    permissions: vec![JrpcPermission::Admin],
    // For simplicity, we're assuming auth is disabled. Use appropriate credentials that match the configured authentication method of your wallet daemon.
    credentials: AuthCredentials::None,
}).await?;
client.set_auth_token(auth_resp.token);

// Create a permission-scoped API key for a non-interactive agent.
let created = client.auth_create_api_key(AuthCreateApiKeyRequest {
    name: "codex-agent".to_string(),
    permissions: vec![JrpcPermission::AccountInfo, JrpcPermission::TransactionGet],
    allow_admin: false,
}).await?;

// Authenticate later with the long-lived API key to receive a short-lived scoped JWT.
client
    .authenticate_with_api_key(vec![JrpcPermission::AccountInfo], created.api_key.to_string())
    .await?;

// Create an account
let resp = client.create_account(AccountsCreateRequest {
    account_name: Some("my-account".to_string()),
    is_default: None,
    key_index: None,
}).await?;

// Get account balances
let balances = client.get_account_balances(AccountsGetBalancesRequest {
    account: Some(ComponentAddressOrName::Name("my-account".to_string())),
    refresh: true,
}).await?;

// Submit a transaction
let resp = client.submit_transaction(TransactionSubmitRequest {
    transaction,
    seal_signer: owner_key_id,
    other_signers: vec![],
    detect_inputs: true,
    detect_inputs_use_unversioned: true,
    lock_ids: vec![],
}).await?;

// Wait for the result
let result = client.wait_transaction_result(TransactionWaitResultRequest {
    transaction_id: resp.transaction_id,
    timeout_secs: Some(120),
}).await?;
```

## Features

- **`ts`** — Generate TypeScript type bindings via `ts-rs`

## License

BSD-3-Clause
