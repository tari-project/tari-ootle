# ootle-wasm

Client-side WebAssembly crypto for [Tari Ootle](https://www.tari.com/) L2. Handles BOR encoding, transaction hashing, Schnorr signing, and key management — with output byte-identical to the native Rust implementation.

## Installation

```bash
npm install @tari-project/ootle-wasm
```

## API

All keys and signatures are **`Uint8Array`** (raw 32-byte values).

### `generateKeypair()`

Generate a new random Ristretto255 keypair.

```typescript
import { generateKeypair } from "@tari-project/ootle-wasm";

const { secret_key, public_key } = generateKeypair();
// secret_key: Uint8Array (32 bytes)
// public_key: Uint8Array (32 bytes)
```

### `publicKeyFromSecretKey(secretKey)`

Derive the public key from a secret key.

```typescript
import { publicKeyFromSecretKey } from "@tari-project/ootle-wasm";

const publicKey = publicKeyFromSecretKey(secretKey);
// publicKey: Uint8Array (32 bytes)
```

### `hashUnsignedTransaction(unsignedTxJson, sealSignerPublicKey)`

Hash an `UnsignedTransactionV1` for signing. Returns a 64-byte `Uint8Array` that should be passed to `schnorrSign`.

- `unsignedTxJson` — JSON-serialised `UnsignedTransactionV1`
- `sealSignerPublicKey` — raw public key bytes of the account owner (seal signer)

```typescript
import { hashUnsignedTransaction } from "@tari-project/ootle-wasm";

const hash = hashUnsignedTransaction(
  JSON.stringify(unsignedTransaction),
  sealSignerPublicKey,
);
```

### `schnorrSign(secretKey, message)`

Schnorr-sign a message (typically the hash from `hashUnsignedTransaction`).

```typescript
import { schnorrSign } from "@tari-project/ootle-wasm";

const { public_nonce, signature } = schnorrSign(secretKey, hash);
// public_nonce: Uint8Array (32 bytes)
// signature:    Uint8Array (32 bytes)
```

### `borEncodeTransaction(transactionJson)`

BOR-encode a signed `Transaction` into a base64 `TransactionEnvelope` string, ready to submit to the network.

```typescript
import { borEncodeTransaction } from "@tari-project/ootle-wasm";

const envelope = borEncodeTransaction(JSON.stringify(transaction));
```

## Working with keys

Keys and signatures are raw `Uint8Array` bytes. Convert to and from hex strings using the built-in methods (Node 22+, modern browsers):

```typescript
import { generateKeypair, publicKeyFromSecretKey } from "@tari-project/ootle-wasm";

// Generate a keypair and display as hex
const { secret_key, public_key } = generateKeypair();
console.log("Public key:", public_key.toHex());

// Load a key from a hex string
const restored = Uint8Array.fromHex("a1b2c3...");
const derivedPublicKey = publicKeyFromSecretKey(restored);
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

// 2. Build an unsigned transaction (application-specific)
const unsignedTx = {
  /* ... UnsignedTransactionV1 fields ... */
};

// 3. Hash for signing
const hash = hashUnsignedTransaction(JSON.stringify(unsignedTx), public_key);

// 4. Sign
const { public_nonce, signature } = schnorrSign(secret_key, hash);

// 5. Assemble the signed transaction and BOR-encode
const signedTx = {
  /* ... Transaction with signature attached ... */
};
const envelope = borEncodeTransaction(JSON.stringify(signedTx));

// 6. Submit envelope to the Ootle network
```

## License

BSD-3-Clause
