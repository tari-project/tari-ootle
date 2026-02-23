# tari_ootle_wallet_crypto

[![Crates.io](https://img.shields.io/crates/v/tari_ootle_wallet_crypto.svg)](https://crates.io/crates/tari_ootle_wallet_crypto)
[![Documentation](https://docs.rs/tari_ootle_wallet_crypto/badge.svg)](https://docs.rs/tari_ootle_wallet_crypto)

Cryptographic primitives for the Tari Ootle wallet. Covers stealth and
confidential transaction construction, AEAD-encrypted UTXO data, Pedersen
commitment handling, balance proofs, and key derivation — everything a wallet
needs to build and interpret privacy-preserving transactions without pulling in
the full engine.

## Modules

| Module                   | Description                                                                                                        |
|--------------------------|--------------------------------------------------------------------------------------------------------------------|
| `stealth`                | Builds `StealthTransferStatement`s: generates output witnesses, extended Bulletproofs, and viewable balance proofs |
| `confidential`           | Creates `ConfidentialWithdrawProof`s for pulling funds out of confidential vaults                                  |
| `encrypted_data`         | XChaCha20-Poly1305 encryption/decryption of UTXO amount and mask (`EncryptedData`)                                 |
| `memo`                   | Structured memo field encoding (U256 value, text message, raw bytes, pay-ref+bytes)                                |
| `pay_to`                 | `PayTo` enum — pay to a stealth public key or an explicit access rule                                              |
| `balance_proof`          | Generates Schnorr balance proof signatures for stealth and confidential transfers                                  |
| `viewable_balance_proof` | ElGamal viewable balance proofs so view-only keys can verify output amounts                                        |
| `kdfs`                   | Domain-separated KDFs for encrypted-data keys and stealth shared secrets                                           |
| `safe_key`               | `SafeAeadKey` — a zeroize-on-drop AEAD key wrapper                                                                 |
| `derive`                 | `derive_ristretto_key` — BIP-style deterministic key derivation from entropy + branch + account index              |

## Key types

| Type                   | Description                                                                       |
|------------------------|-----------------------------------------------------------------------------------|
| `StealthCryptoApi`     | Stateless API struct — entry point for stealth transfer and decryption operations |
| `OutputWitness`        | Unblinded representation of a UTXO: amount, mask, nonce, encrypted data           |
| `StealthOutputWitness` | `OutputWitness` plus a spend condition and UTXO tag                               |
| `MaskAndValue`         | A (mask, value) pair representing a Pedersen input commitment                     |
| `DecryptedData`        | Result of decrypting a UTXO's `EncryptedData`: amount, mask, optional memo        |

## Example

```rust
use tari_ootle_wallet_crypto::derive_ristretto_key;

// Derive a deterministic Ristretto secret key from wallet entropy
let entropy = [0u8; 64]; // from BIP-39 seed or similar
let key = derive_ristretto_key( & entropy, b"spend", 0);
```

## License

BSD-3-Clause. Copyright 2023 The Tari Project.
