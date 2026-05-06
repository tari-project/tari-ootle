# Tari Ootle Wallet JSON-RPC Client

## Overview

Client library for interacting with the Tari Ootle Wallet Daemon via JSON-RPC. 

## Agent API Keys

Admin clients can create, list, and revoke permission-scoped API keys:

```ts
const created = await client.authCreateApiKey({
  name: "codex-agent",
  permissions: ["AccountInfo", "TransactionGet"],
  allow_admin: false,
});

await client.authenticateWithApiKey(["AccountInfo"], created.api_key);
```

The API key is shown once at creation; the wallet daemon persists only its hash.
