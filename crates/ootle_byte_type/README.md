# ootle_byte_type

[![Crates.io](https://img.shields.io/crates/v/ootle_byte_type.svg)](https://crates.io/crates/ootle_byte_type)
[![Documentation](https://docs.rs/ootle_byte_type/badge.svg)](https://docs.rs/ootle_byte_type)

Lightweight conversion traits between rich types and their flat fixed byte-array
representations. Useful for serialisation boundaries where allocating or
expensive Edwards point decompression operations are undesirable.

## Traits

| Trait                    | Direction             | Method                                                 |
|--------------------------|-----------------------|--------------------------------------------------------|
| `ToByteType`             | `T → Bytes`           | `to_byte_type(&self) -> Self::ByteType`                |
| `ConvertFromByteType<B>` | `Bytes → T`           | `convert_from_byte_type(bytes: &B) -> Result<Self, _>` |
| `FromByteType<T>`        | `Bytes → T` (blanket) | `try_from_byte_type(&self) -> Result<T, _>`            |

`FromByteType` is automatically implemented for any `B` where `T: ConvertFromByteType<B>`,
so you only need to implement `ConvertFromByteType`.

## Example

```rust
use ootle_byte_type::{ToByteType, FromByteType};
use tari_crypto::{keys::PublicKey, ristretto::RistrettoPublicKey};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

// Rich type → bytes
let (_, pk) = RistrettoPublicKey::random_keypair( & mut rand::thread_rng());
let bytes: RistrettoPublicKeyBytes = pk.to_byte_type();

// Bytes → rich type
let pk2: RistrettoPublicKey = bytes.try_from_byte_type().unwrap();
assert_eq!(pk, pk2);
```

With the `crypto` feature (enabled by default) implementations are provided for
`RistrettoPublicKey`, `PedersenCommitment`, `SchnorrSignature`,
`CommitmentSignature`, and `Option<T>`.

## License

BSD-3-Clause. Copyright 2026 The Tari Project.
