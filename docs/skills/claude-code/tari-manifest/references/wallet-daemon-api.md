# Wallet Daemon JSON-RPC API Reference

The wallet daemon exposes a JSON-RPC 2.0 API. All requests are POST to the endpoint (e.g. `http://localhost:12008/json_rpc`).

## Authentication

### Auth Methods

The daemon supports two authentication modes configured via `WalletDaemonAuth`:

- **`none`** (default) - Anonymous authentication. Any client can obtain a JWT by sending `"credentials": "None"`.
- **`webauthn`** - WebAuthn/passkey-based authentication. Requires a registered passkey to authenticate.

### Authentication Flow (auth.method = "none")

1. **Check method:** `auth.method` (no auth required)
2. **Login:** `auth.request` with `{"permissions": ["Admin"], "credentials": "None"}`
3. **Use token:** Include `Authorization: Bearer <token>` on all subsequent requests
4. **Refresh:** When token expires (default 5 min), call `auth.refresh` (uses HttpOnly cookie set during login)

### Authentication Flow (auth.method = "webauthn")

WebAuthn requires a browser-based passkey interaction (biometric, hardware key, etc.) that **cannot be performed by an agent directly**. The full protocol flow is:

1. **Check method:** `auth.method`
2. **Start auth challenge:** `webauthn.auth_start` with `{"username": "..."}`
3. **Complete auth:** Use the challenge to produce a WebAuthn credential response (requires browser/passkey device)
4. **Login:** `auth.request` with `{"permissions": [...], "credentials": {"WebAuthN": <response>}}`
5. **Use token:** Same as above

**For agents:** Ask an Admin user to create a permission-scoped API key in the wallet UI or with `tari_wallet_cli auth api-key create`. Use the API key with `auth.request` credentials `{"ApiKey":{"api_key":"..."}}` to receive a short-lived JWT scoped to the key. If the JWT expires, repeat `auth.request` with the same API key.

### JWT Token Details

- Algorithm: HS256
- Default expiry: 5 minutes (configurable via `jwt_expiry`)
- Claims: `{ "permissions": [...], "exp": <unix_timestamp> }`
- Refresh tokens: Stored as HttpOnly, SameSite=Strict cookies (`r-tkn`), expire after 1 hour

### Permissions

```
Admin              - Full access (includes all other permissions)
AccountInfo        - View wallet info
AccountList        - List accounts (optionally scoped to a component address)
AccountBalance     - View balances (scoped to a substate ID)
KeyList            - List derived keys
TransactionGet     - View transactions
TransactionSend    - Submit transactions (optionally scoped to a substate ID)
SubstatesRead      - Fetch substates
TemplatesRead      - Fetch templates
GetNft             - View NFTs
NftGetOwnershipProof - Generate NFT ownership proofs
StartWebrtc        - Initiate WebRTC sessions (UI internal use)
```

Use `Admin` for full access. For least-privilege, use `TransactionSend` + `AccountList` + `AccountInfo`.

## Auth Endpoints

### auth.method

Check the configured authentication method. No token required.

**Request:** `{"jsonrpc":"2.0","id":1,"method":"auth.method","params":{}}`

**Response:** `{"result":{"method":"none"}}` or `{"result":{"method":"webauthn"}}`

### auth.request

Authenticate and receive a JWT token.

**Request:**
```json
{
  "jsonrpc":"2.0","id":2,"method":"auth.request",
  "params":{
    "permissions": ["Admin"],
    "credentials": "None"
  }
}
```

For WebAuthn: `"credentials": {"WebAuthN": <WebauthnFinishAuthRequest>}`

For API keys: `"credentials": {"ApiKey": {"api_key": "twda_<id>_<secret>"}}`

**Response:** `{"result":{"token":"eyJ..."}}`

Also sets an HttpOnly `r-tkn` cookie for refresh.

### auth.api_key_create

Create a named API key. Requires `Admin` permission. The API key is returned only in this response; the wallet stores only its hash.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "auth.api_key_create",
  "params": {
    "name": "codex-agent",
    "permissions": ["AccountInfo", "TransactionGet"],
    "allow_admin": false
  }
}
```

Set `allow_admin` to `true` only when `permissions` includes `Admin`.

### auth.api_key_list

List active API keys. Requires `Admin` permission. Returns each key's `id`, `name`, `permissions`, `created_at`, and `last_used_at`.

### auth.api_key_revoke

Revoke an active API key by id. Requires `Admin` permission. Revocation is immediate for new and existing JWT checks.

### auth.refresh

Refresh an expired JWT token using the refresh cookie.

**Request:** `{"jsonrpc":"2.0","id":3,"method":"auth.refresh","params":{}}`

**Response:** `{"result":{"token":"eyJ..."}}`

### auth.revoke

Revoke a refresh token. Requires `Admin` permission.

**Request:** `{"jsonrpc":"2.0","id":4,"method":"auth.revoke","params":{"refresh_token_id":"<hash>"}}`

### auth.list_sessions

List all active sessions. Requires `Admin` permission.

**Request:** `{"jsonrpc":"2.0","id":5,"method":"auth.list_sessions","params":{}}`

## Transaction Endpoints

**CRITICAL: Before submitting any transaction (with `dry_run: false`), ALWAYS present a plain-language summary to the user and wait for explicit confirmation. Dry-run submissions do not require confirmation.**

### transactions.submit_manifest

Submit a transaction defined by a manifest.

**Request:**
```json
{
  "jsonrpc":"2.0","id":10,"method":"transactions.submit_manifest",
  "params":{
    "manifest": "<manifest source code>",
    "variables": {
      "account": "component_<64-char-hex>",
      "resource": "resource_<64-char-hex>"
    },
    "max_fee": 1000,
    "dry_run": false
  }
}
```

**Fields:**
- `manifest` (string, required): Manifest source code. Escape inner quotes, use `\n` for newlines.
- `variables` (object, required): Variable name to value string mapping. Values are parsed as `ManifestValue` (tries SubstateId, then NonFungibleId, then literal).
- `signing_key_id` (object, optional): `{"Derived":{"index":0,"key_branch":"account"}}` to override signing key.
- `max_fee` (number, required): Maximum fee in microtari.
- `dry_run` (boolean, required): If true, simulate without submitting.

**Response:**
```json
{
  "result": {
    "transaction_id": "<hex>",
    "result": null
  }
}
```

For `dry_run: true`, `result` contains the `ExecuteResult` with logs, outputs, etc.

### transactions.submit

Submit a pre-built transaction with raw instructions.

**Request:**
```json
{
  "jsonrpc":"2.0","id":11,"method":"transactions.submit",
  "params":{
    "transaction": { ... },
    "signing_key_id": null,
    "detect_inputs": true,
    "detect_inputs_use_unversioned": true
  }
}
```

### transactions.wait_result

Block until a transaction is finalized or timeout.

**Request:**
```json
{
  "jsonrpc":"2.0","id":12,"method":"transactions.wait_result",
  "params":{
    "transaction_id": "<hex>",
    "timeout_secs": 120
  }
}
```

### transactions.get_result

Non-blocking poll for transaction result.

**Request:**
```json
{
  "jsonrpc":"2.0","id":13,"method":"transactions.get_result",
  "params":{"transaction_id":"<hex>"}
}
```

## Account Endpoints

### accounts.list

**Request:** `{"jsonrpc":"2.0","id":20,"method":"accounts.list","params":{"offset":0,"limit":10}}`

### accounts.get_default

**Request:** `{"jsonrpc":"2.0","id":21,"method":"accounts.get_default","params":{}}`

### accounts.get

**Request:** `{"jsonrpc":"2.0","id":22,"method":"accounts.get","params":{"name_or_address":"my-account"}}`

Can use account name or component address.

### accounts.get_balances

**Request:** `{"jsonrpc":"2.0","id":23,"method":"accounts.get_balances","params":{"account":"my-account","refresh":true}}`

### accounts.create_free_test_coins

Create free test coins (test networks only).

**Request:**
```json
{
  "jsonrpc":"2.0","id":24,"method":"accounts.create_free_test_coins",
  "params":{
    "account": {"Name":"my-account"},
    "amount": 1000000,
    "max_fee": 1000
  }
}
```

## Substate and Template Endpoints

### substates.get

**Request:** `{"jsonrpc":"2.0","id":30,"method":"substates.get","params":{"substate_id":"component_<hex>"}}`

### templates.get

**Request:** `{"jsonrpc":"2.0","id":31,"method":"templates.get","params":{"template_address":"<hex>"}}`

Returns template function definitions including argument types - useful for knowing what methods a component supports.

## Source Code Locations

- Wallet daemon: `applications/tari_walletd/`
- Auth handlers: `applications/tari_walletd/src/handlers/auth/`
- Transaction handlers: `applications/tari_walletd/src/handlers/transaction.rs`
- Client library: `clients/wallet_daemon_client/src/lib.rs`
- Client types: `clients/wallet_daemon_client/src/types.rs`
- Permissions: `clients/wallet_daemon_client/src/permissions.rs`
- Config: `applications/tari_walletd/src/config.rs`
