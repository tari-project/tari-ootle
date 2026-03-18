# ootle-wasm

Client-side WebAssembly crypto for [Tari Ootle](https://www.tari.com/) L2. Handles BOR encoding, transaction hashing, Schnorr signing, and key management — with output byte-identical to the native Rust implementation.

## Installation

```bash
npm install @tari-project/ootle-wasm
```

## API

All keys and signatures are **lowercase hex-encoded strings**.

### `generateKeypair()`

Generate a new random Ristretto255 keypair.

```typescript
import { generateKeypair } from "@tari-project/ootle-wasm";

const { secret_key, public_key } = generateKeypair();
// secret_key: "a1b2c3..." (64 hex chars)
// public_key: "d4e5f6..." (64 hex chars)
```

### `publicKeyFromSecretKey(secretKeyHex)`

Derive the public key from a secret key.

```typescript
import { publicKeyFromSecretKey } from "@tari-project/ootle-wasm";

const publicKey = publicKeyFromSecretKey(secretKeyHex);
```

### `hashUnsignedTransaction(unsignedTxJson, sealSignerPublicKeyHex)`

Hash an `UnsignedTransactionV1` for signing. Returns a 64-byte `Uint8Array` that should be passed to `schnorrSign`.

- `unsignedTxJson` — JSON-serialised `UnsignedTransactionV1`
- `sealSignerPublicKeyHex` — hex-encoded public key of the account owner (seal signer)

```typescript
import { hashUnsignedTransaction } from "@tari-project/ootle-wasm";

const hash = hashUnsignedTransaction(
  JSON.stringify(unsignedTransaction),
  sealSignerPublicKeyHex,
);
```

### `schnorrSign(secretKeyHex, message)`

Schnorr-sign a message (typically the hash from `hashUnsignedTransaction`).

```typescript
import { schnorrSign } from "@tari-project/ootle-wasm";

const { public_nonce, signature } = schnorrSign(secretKeyHex, hash);
// public_nonce: hex string
// signature:    hex string
```

### `borEncodeTransaction(transactionJson)`

BOR-encode a signed `Transaction` into a base64 `TransactionEnvelope` string, ready to submit to the network.

```typescript
import { borEncodeTransaction } from "@tari-project/ootle-wasm";

const envelope = borEncodeTransaction(JSON.stringify(transaction));
```

## Full example

```typescript
import {
  generateKeypair,
  publicKeyFromSecretKey,
  hashUnsignedTransaction,
  schnorrSign,
  borEncodeTransaction,
} from "@tari-project/ootle-wasm";

// 1. Generate or load a keypair
const { secret_key, public_key } = generateKeypair();

// 3. Build an unsigned transaction (application-specific)
const unsignedTx = {
  /* ... UnsignedTransactionV1 fields ... */
};

// 4. Hash for signing
const hash = hashUnsignedTransaction(JSON.stringify(unsignedTx), public_key);

// 5. Sign
const { public_nonce, signature } = schnorrSign(secret_key, hash);

// 6. Assemble the signed transaction and BOR-encode
const signedTx = {
  /* ... Transaction with signature attached ... */
};
const envelope = borEncodeTransaction(JSON.stringify(signedTx));

// 7. Submit envelope to the Ootle network
```

## License

BSD-3-Clause
