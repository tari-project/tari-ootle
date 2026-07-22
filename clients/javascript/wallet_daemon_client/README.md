# Tari Ootle Wallet JSON-RPC Client

## Overview

Client library for interacting with the Tari Ootle Wallet Daemon via JSON-RPC.

## Authentication

The daemon accepts two kinds of bearer credential, and which one you can use depends on where your code runs.

### Same-origin browser callers — session tokens

A page served by the daemon itself authenticates with `auth.request`, which returns a short-lived JWT and sets an
HttpOnly, `SameSite=Strict` refresh cookie. With `setReauthenticationEnabled(true)`, an expired JWT is refreshed
transparently via `auth.refresh`.

```ts
const client = WalletDaemonClient.usingFetchTransport(WALLET_JRPC_URL);
const token = await client.authRequest(permissions, credentials);
client.setToken(token);
```

`auth.refresh` is authorised by the cookie alone — the bearer token is not consulted. Because the cookie is HttpOnly
and `SameSite=Strict`, only a same-origin caller can hold a refreshable session.

### Cross-origin callers — API keys

Browser extensions, other hosts, and non-browser tooling cannot obtain the refresh cookie and so cannot hold a
session. Authenticate with an API key instead: it is validated against the daemon's `api_keys` table on every call,
with no `auth.request` round-trip and nothing to refresh.

```ts
const client = WalletDaemonClient.usingFetchTransport(WALLET_JRPC_URL);
client.authenticateWithApiKey(apiKey);
client.setReauthenticationEnabled(false);
```

A 401 from an API key is final — the key is revoked, expired, or lacks the permission the method requires.

Keys are minted with `auth.create_api_key`, which requires an interactive Admin session; an API key cannot mint
further keys. Scope each key to the narrowest permission set that works — a client that only proposes transactions
for a human to approve needs `TransactionRequests(Create)` and not `Transfer` or `Transactions(Create)`.

The daemon only serves cross-origin callers when its CORS configuration permits the origin.

### Transport cookie policy

`FetchRpcTransport` sends cookies according to its `credentials` option, which defaults to `"same-origin"`:

```ts
WalletDaemonClient.usingFetchTransport(url, { credentials: "include" });
```
