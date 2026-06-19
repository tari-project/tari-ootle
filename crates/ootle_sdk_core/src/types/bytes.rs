//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Boundary-facing scalar/byte newtypes.
//!
//! Each carries raw bytes (a fixed array for known widths, otherwise a `Vec<u8>`) and serializes as
//! **lowercase hex, no `0x` prefix** — a stable, language-neutral representation every other SDK can
//! read. Fixed widths match the corresponding internal crypto types (a Ristretto public key is 32
//! bytes).

use serde::{Deserialize, Serialize};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::types::error::OotleSdkError;

/// The width of a Ristretto public key / secret key / 32-byte scalar.
pub const RISTRETTO_KEY_LEN: usize = 32;

/// serde helper: a fixed-width byte array as lowercase hex.
mod fixed_hex {
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};

    pub fn serialize<S: Serializer, const N: usize>(bytes: &[u8; N], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>, const N: usize>(d: D) -> Result<[u8; N], D::Error> {
        let s = String::deserialize(d)?;
        if s.chars().any(|c| c.is_ascii_uppercase()) {
            return Err(D::Error::custom("expected lowercase hex"));
        }
        let v = hex::decode(&s).map_err(D::Error::custom)?;
        let arr: [u8; N] = v
            .try_into()
            .map_err(|v: Vec<u8>| D::Error::custom(format!("expected {N} bytes, got {}", v.len())))?;
        Ok(arr)
    }
}

/// serde helper: a variable-length byte vector as lowercase hex.
mod var_hex {
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        if s.chars().any(|c| c.is_ascii_uppercase()) {
            return Err(D::Error::custom("expected lowercase hex"));
        }
        hex::decode(&s).map_err(D::Error::custom)
    }
}

/// Generates a fixed-width byte newtype that serializes as lowercase hex.
macro_rules! fixed_byte_newtype {
    ($(#[$meta:meta])* $name:ident, $len:expr) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(#[serde(with = "fixed_hex")] pub [u8; $len]);

        impl $name {
            /// The fixed byte width.
            pub const LEN: usize = $len;

            /// Wraps a fixed-width byte array.
            pub const fn from_array(bytes: [u8; $len]) -> Self {
                Self(bytes)
            }

            /// Builds from a byte slice, erroring on a width mismatch.
            pub fn from_bytes(bytes: &[u8]) -> Result<Self, OotleSdkError> {
                let arr: [u8; $len] = bytes.try_into().map_err(|_| {
                    OotleSdkError::Validation(format!(
                        concat!(stringify!($name), ": expected {} bytes, got {}"),
                        $len,
                        bytes.len()
                    ))
                })?;
                Ok(Self(arr))
            }

            /// Parses from a lowercase-hex string.
            pub fn from_hex(s: &str) -> Result<Self, OotleSdkError> {
                let v = hex::decode(s).map_err(|e| OotleSdkError::Parse(format!(
                    concat!(stringify!($name), ": invalid hex: {}"), e
                )))?;
                Self::from_bytes(&v)
            }

            /// Borrows the raw bytes.
            pub fn as_bytes(&self) -> &[u8] {
                &self.0
            }

            /// Returns the owned byte array.
            pub const fn into_array(self) -> [u8; $len] {
                self.0
            }

            /// Returns the lowercase-hex encoding.
            pub fn to_hex(&self) -> String {
                hex::encode(self.0)
            }
        }
    };
}

/// Generates a fixed-width *secret-material* byte newtype.
///
/// Identical to [`fixed_byte_newtype`] but deliberately omits `Copy` and `Hash`: secret material
/// should not be silently duplicated by the compiler, nor used as a map key (a hashing side-channel).
///
/// **Zeroize-on-drop guarantee:** the raw bytes are wiped from memory when the value drops.
/// The struct derives [`zeroize::Zeroize`] (the inner `[u8; N]` is zeroizable) and implements
/// [`zeroize::ZeroizeOnDrop`] via an explicit `Drop` that calls `self.zeroize()`. This makes every
/// caller-supplied spend/account secret (parsed into one of these newtypes) un-readable in freed
/// heap/stack after it goes out of scope, instead of lingering for a later heap read / core dump /
/// swap. `zeroize` performs no `unsafe` here.
macro_rules! secret_byte_newtype {
    ($(#[$meta:meta])* $name:ident, $len:expr) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, zeroize::Zeroize)]
        pub struct $name(#[serde(with = "fixed_hex")] pub [u8; $len]);

        // Wipe the secret bytes on drop. `ZeroizeOnDrop` is the marker trait; the `Drop` impl does the
        // actual wipe (deriving `ZeroizeOnDrop` would synthesize an equivalent `Drop`; it is written
        // explicitly so the wipe is obvious at the definition site). `zeroize` performs no `unsafe` here.
        impl zeroize::ZeroizeOnDrop for $name {}
        impl Drop for $name {
            fn drop(&mut self) {
                use zeroize::Zeroize as _;
                self.0.zeroize();
            }
        }

        impl $name {
            /// The fixed byte width.
            pub const LEN: usize = $len;

            /// Wraps a fixed-width byte array.
            pub const fn from_array(bytes: [u8; $len]) -> Self {
                Self(bytes)
            }

            /// Builds from a byte slice, erroring on a width mismatch.
            pub fn from_bytes(bytes: &[u8]) -> Result<Self, OotleSdkError> {
                let arr: [u8; $len] = bytes.try_into().map_err(|_| {
                    OotleSdkError::Validation(format!(
                        concat!(stringify!($name), ": expected {} bytes, got {}"),
                        $len,
                        bytes.len()
                    ))
                })?;
                Ok(Self(arr))
            }

            /// Parses from a lowercase-hex string.
            pub fn from_hex(s: &str) -> Result<Self, OotleSdkError> {
                let v = hex::decode(s).map_err(|e| OotleSdkError::Parse(format!(
                    concat!(stringify!($name), ": invalid hex: {}"), e
                )))?;
                Self::from_bytes(&v)
            }

            /// Borrows the raw bytes.
            pub fn as_bytes(&self) -> &[u8] {
                &self.0
            }

            /// Returns a copy of the byte array.
            ///
            /// `[u8; N]` is `Copy`, so this reads the bytes out *by copy* (it cannot move `self.0`
            /// out of a `ZeroizeOnDrop` type). The original `self` is then dropped and its bytes are
            /// wiped — but the returned copy is **not** zeroized and is the caller's responsibility.
            pub fn into_array(self) -> [u8; $len] {
                self.0
            }

            /// Returns the lowercase-hex encoding.
            pub fn to_hex(&self) -> String {
                hex::encode(self.0)
            }
        }
    };
}

/// Generates a variable-length byte newtype that serializes as lowercase hex.
macro_rules! var_byte_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(#[serde(with = "var_hex")] pub Vec<u8>);

        impl $name {
            /// Wraps an owned byte vector.
            pub const fn from_vec(bytes: Vec<u8>) -> Self {
                Self(bytes)
            }

            /// Copies a byte slice into a new value.
            pub fn from_bytes(bytes: &[u8]) -> Self {
                Self(bytes.to_vec())
            }

            /// Parses from a lowercase-hex string.
            pub fn from_hex(s: &str) -> Result<Self, OotleSdkError> {
                let v = hex::decode(s).map_err(|e| OotleSdkError::Parse(format!(
                    concat!(stringify!($name), ": invalid hex: {}"), e
                )))?;
                Ok(Self(v))
            }

            /// Borrows the raw bytes.
            pub fn as_bytes(&self) -> &[u8] {
                &self.0
            }

            /// Returns the owned byte vector.
            pub fn into_vec(self) -> Vec<u8> {
                self.0
            }

            /// Returns the lowercase-hex encoding.
            pub fn to_hex(&self) -> String {
                hex::encode(&self.0)
            }
        }
    };
}

fixed_byte_newtype!(
    /// A Ristretto public key (32 bytes), boundary form.
    PublicKeyBytes,
    RISTRETTO_KEY_LEN
);
secret_byte_newtype!(
    /// A Ristretto secret key (32 bytes), boundary form. Crosses the boundary only as an explicit
    /// caller-supplied key. Secret material — no `Copy`/`Hash`.
    SecretKeyBytes,
    RISTRETTO_KEY_LEN
);
secret_byte_newtype!(
    /// A nonce secret (32 bytes) supplied to the deterministic-seal path. Secret material — no
    /// `Copy`/`Hash`.
    NonceSecretBytes,
    RISTRETTO_KEY_LEN
);

var_byte_newtype!(
    /// A Schnorr signature, boundary form (variable to stay agnostic to the internal layout).
    SignatureBytes
);
var_byte_newtype!(
    /// A transaction id, boundary form.
    TransactionIdBytes
);
var_byte_newtype!(
    /// The submit-ready BOR-encoded transaction bytes.
    EncodedTransactionBytes
);

impl PublicKeyBytes {
    /// Builds from the internal [`RistrettoPublicKeyBytes`].
    pub fn from_internal(pk: &RistrettoPublicKeyBytes) -> Self {
        let mut arr = [0u8; RISTRETTO_KEY_LEN];
        arr.copy_from_slice(pk.as_bytes());
        Self(arr)
    }

    /// Converts to the internal [`RistrettoPublicKeyBytes`].
    pub fn to_internal(&self) -> RistrettoPublicKeyBytes {
        // Width is guaranteed by the fixed-array newtype, so this never fails.
        RistrettoPublicKeyBytes::from(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_newtype_hex_round_trip() {
        let pk = PublicKeyBytes::from_array([7u8; 32]);
        let h = pk.to_hex();
        assert_eq!(h.len(), 64);
        assert_eq!(PublicKeyBytes::from_hex(&h).unwrap(), pk);
    }

    #[test]
    fn fixed_newtype_rejects_wrong_width() {
        let err = PublicKeyBytes::from_bytes(&[1u8; 31]).unwrap_err();
        assert_eq!(err.code(), "VALIDATION");
        let err = PublicKeyBytes::from_hex("zz").unwrap_err();
        assert_eq!(err.code(), "PARSE");
    }

    #[test]
    fn fixed_newtype_serializes_as_hex_string() {
        let json = serde_json::to_string(&PublicKeyBytes::from_array([0xab; 32])).unwrap();
        assert_eq!(json, format!("\"{}\"", "ab".repeat(32)));
    }

    #[test]
    fn var_newtype_hex_round_trip() {
        let sig = SignatureBytes::from_bytes(&[1, 2, 3, 4, 5]);
        assert_eq!(SignatureBytes::from_hex(&sig.to_hex()).unwrap(), sig);
        let json = serde_json::to_string(&sig).unwrap();
        assert_eq!(json, "\"0102030405\"");
    }

    #[test]
    fn deserialize_rejects_uppercase_hex() {
        // Fixtures are lowercase-only; uppercase must fail to keep serde round-trips stable.
        let err = serde_json::from_str::<PublicKeyBytes>(&format!("\"{}\"", "AB".repeat(32)));
        assert!(err.is_err());
        let err = serde_json::from_str::<SignatureBytes>("\"0A0B\"");
        assert!(err.is_err());
    }

    #[test]
    fn secret_newtype_round_trips() {
        let sk = SecretKeyBytes::from_array([9u8; 32]);
        assert_eq!(SecretKeyBytes::from_hex(&sk.to_hex()).unwrap(), sk);
        let nonce = NonceSecretBytes::from_array([4u8; 32]);
        let json = serde_json::to_string(&nonce).unwrap();
        assert_eq!(serde_json::from_str::<NonceSecretBytes>(&json).unwrap(), nonce);
    }

    #[test]
    fn public_key_internal_round_trip() {
        let internal = RistrettoPublicKeyBytes::from([3u8; 32]);
        let boundary = PublicKeyBytes::from_internal(&internal);
        assert_eq!(boundary.to_internal(), internal);
    }

    // --- zeroize-on-drop wiring -------------------------------------------------------------------

    /// Compile-time proof the secret newtypes are wired to `ZeroizeOnDrop`. If a future edit drops
    /// the marker trait / `Drop` impl from the macro, this stops compiling.
    fn _assert_zeroize_on_drop<T: zeroize::ZeroizeOnDrop>() {}

    /// Compile-time witness that `tari_crypto`'s `RistrettoSecretKey` is `ZeroizeOnDrop`, so
    /// recovered stealth masks held as `RistrettoSecretKey`s wipe on drop without a wrapper.
    fn _assert_ristretto_secret_zeroizes() {
        _assert_zeroize_on_drop::<tari_crypto::ristretto::RistrettoSecretKey>();
    }

    #[test]
    fn secret_newtypes_are_zeroize_on_drop() {
        // Trait-bound assertions: the caller-supplied spend/account secrets wipe on drop.
        _assert_zeroize_on_drop::<SecretKeyBytes>();
        _assert_zeroize_on_drop::<NonceSecretBytes>();
        _assert_ristretto_secret_zeroizes();
    }

    #[test]
    fn secret_newtype_zeroize_clears_the_bytes() {
        use zeroize::Zeroize as _;

        let mut sk = SecretKeyBytes::from_array([0xACu8; 32]);
        assert_eq!(sk.as_bytes(), &[0xAC; 32]);
        // The same wipe the `Drop` impl runs — exercised directly so we can observe the result.
        sk.zeroize();
        assert_eq!(sk.as_bytes(), &[0u8; 32], "zeroize must clear every secret byte");

        let mut nonce = NonceSecretBytes::from_array([0x5Au8; 32]);
        nonce.zeroize();
        assert_eq!(nonce.as_bytes(), &[0u8; 32]);
    }
}
