# Wallet Daemon JSON-RPC API Reference

The wallet daemon exposes a JSON-RPC 2.0 API. All requests are POST to the endpoint (e.g. `http://127.0.0.1:5100/json_rpc`).

## Authentication

### Auth Methods

The daemon supports two authentication modes configured via `WalletDaemonAuth`:

- **`none`** - Anonymous authentication. Any client can obtain a JWT by sending `"credentials": "None"`.
- **`webauthn`** (default) - WebAuthn/passkey-based authentication. Requires a registered passkey to authenticate.

### Authentication Flow (auth.method = "none")

1. **Check method:** `auth.method` (no auth required)
2. **Login:** `auth.request` with `{"permissions": ["Admin"], "credentials": "None"}`
3. **Use token:** Include `Authorization: Bearer <token>` on all subsequent requests
4. **Refresh:** When token expires (default 5 min), call `auth.refresh` (uses HttpOnly cookie set during login)

### Authentication Flow (auth.method = "webauthn")

WebAuthn requires a browser-based passkey interaction (biometric, hardware key, etc.) that **cannot be performed by an agent directly**. The full protocol flow is:

1. **Check method:** `auth.method`
2. **Start auth challenge:** `webauthn.auth_start` with `{}`
3. **Complete auth:** Use the challenge to produce a WebAuthn credential response (requires browser/passkey device)
4. **Login:** `auth.request` with `{"permissions": [...], "credentials": {"WebAuthN": <response>}}`
5. **Use token:** Same as above

**For agents:** Ask the user to authenticate via the wallet daemon web UI and provide the JWT token. Once received, use it in `Authorization: Bearer <token>` headers. If the token expires, ask the user for a fresh one.

### JWT Token Details

- Algorithm: HS256
- Default expiry: 5 minutes (configurable via `jwt_expiry`)
- Claims: `{ "permissions": [...], "exp": <unix_timestamp> }`
- Refresh tokens: Stored as HttpOnly, SameSite=Strict cookies (`r-tkn`), expire after 1 hour

### Permissions

`auth.request` `permissions` is the serde form of the `Permission` enum. `"Admin"` grants every permission. The other values:

```
"Webrtc"
{"Accounts":["Read", null]}           - action is Read, Create, Update, or Delete; the second element is a component address or null
{"Keys":"Read"}                       - Read, Create, Update, or Delete
{"Transactions":"Create"}             - Read, Create, Update, or Delete. transactions.submit_manifest requires Create
{"TransactionRequests":"Approve"}     - Read, Create, or Approve
{"SigningRequests":"Read"}            - Read, Create, or Approve
{"Transfer":["Create", null]}         - same shape as Accounts
{"Templates":"Read"}                  - Read, Create, Update, or Delete
{"Nfts":["Read", null]}               - action as Accounts; the scope is a resource address or null
{"Confidential":["Read", null]}       - same shape as Accounts
{"StealthUtxos":["Read", null]}       - same shape as Accounts
{"Validators":"Read"}                 - Read, Create, Update, or Delete
{"Settings":"Read"}                   - Read, Create, Update, or Delete
{"AddressBook":"Read"}                - Read, Create, Update, or Delete
{"Substates":"Read"}
{"BurnProofs":"Read"}
{"SwapPools":"Read"}
{"Resources":"Read"}
```

Names such as `AccountInfo` and `TransactionSend` are not accepted. API keys use a separate grammar, `admin` or `<resource>:<action>[:<entity>]` (for example `accounts:read`, `transactions:create`). Those strings are not the `auth.request` form.

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

**Response:** `{"result":{"token":"eyJ..."}}`

Also sets an HttpOnly `r-tkn` cookie for refresh.

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
    "seal_signer_key_id": null,
    "max_fee": 1000,
    "dry_run": false
  }
}
```

**Fields:**
- `manifest` (string, required): Manifest source code. Escape inner quotes, use `\n` for newlines.
- `variables` (object, required): Variable name to value string mapping. Values are parsed as `ManifestValue` (tries SubstateId, then NonFungibleId, then literal).
- `seal_signer_key_id` (object or null, required): Key that seals the transaction. `null` uses the default account's owner key. Example: `{"Derived":{"key_branch":"account","index":0}}`.
- `signing_key_ids` (array, optional): Additional signing keys. Defaults to `[]`.
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
    "seal_signer": {"Derived":{"key_branch":"account","index":0}},
    "other_signers": [],
    "detect_inputs": true,
    "detect_inputs_use_unversioned": true,
    "lock_ids": []
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

Create free test coins (test networks only). The claim amount is fixed at 1,000 TARI; the request has no amount field.

**Request:**
```json
{
  "jsonrpc":"2.0","id":24,"method":"accounts.create_free_test_coins",
  "params":{
    "account": {"Name":"my-account"},
    "max_fee": 1000
  }
}
```

### accounts.get_faucet_balance

The testnet faucet's balance and the amount one claim takes. Public testnets start with an empty faucet; a claim
fails with error code `1002` while `balance` is below `claim_amount`. Returns a not-found error (`404`) when the
indexer has no faucet vault, e.g. on mainnet.

**Request:** `{"jsonrpc":"2.0","id":25,"method":"accounts.get_faucet_balance","params":{}}`

**Response:** `{"balance": "0", "claim_amount": "1000000000"}` (amounts are decimal strings)

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
