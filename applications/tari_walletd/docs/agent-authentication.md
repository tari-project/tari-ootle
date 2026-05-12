# Agent authentication

## Overview

The wallet daemon supports long-lived API keys so AI agents can authenticate
without interactive user presence. An admin creates a key once; the agent
exchanges it for a short-lived JWT and re-exchanges automatically.

## Prerequisites

You must hold an active session with the Admin permission.

## Creating an API key

### Via JSON-RPC

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "api_keys.create",
  "params": {
    "name": "claude-agent-payments",
    "permissions": ["AccountInfo", "TransactionSend"],
    "grant_admin": false
  }
}
```

The response contains a `key` field. **Copy it now; it is never shown again.**

### Via Rust client

```rust
use tari_ootle_walletd_client::permissions::JrpcPermission;

let response = client
    .create_api_key(
        "claude-agent-payments",
        vec![
            JrpcPermission::AccountInfo,
            JrpcPermission::TransactionSend(None),
        ],
        false,
    )
    .await?;

// Store response.key securely. It is shown only once.
```

### Via JavaScript client

```typescript
const response = await client.createApiKey(
  "claude-agent-payments",
  ["AccountInfo", "TransactionSend"],
  false
);

// Store response.key securely. It is shown only once.
```

## Agent authentication flow

### Rust

```rust
client.authenticate_with_api_key(&stored_key).await?;
// JWT is now stored; subsequent calls are authenticated
// Re-call authenticate_with_api_key every ~14 minutes
```

### JavaScript

```typescript
await client.authenticateWithApiKey(storedKey);
// Re-call every ~14 minutes before JWT expires
```

## Available scopes

| Permission | Description | Access |
|---|---|---|
| AccountInfo | Read account details | read |
| AccountBalance | Read account balance | read |
| AccountList | List accounts | read |
| KeyList | List keys | read |
| TransactionGet | Read transactions | read |
| TransactionSend | Submit transactions | write |
| SubstatesRead | Read substates | read |
| TemplatesRead | Read templates | read |
| StartWebrtc | Start WebRTC session | write |
| Admin | Full wallet access | write |

## Security notes

- The plaintext key is shown exactly once at creation. Store it in a secret manager.
- Only the SHA-256 hash is persisted in the wallet database.
- Revoking a key immediately blocks new JWT issuance. Already-issued JWTs remain valid until their 15-minute expiry.
- Granting Admin scope requires passing `grant_admin: true` explicitly.
