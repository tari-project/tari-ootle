//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[macro_export]
macro_rules! create_hash_type {
    (
         $(#[$meta:meta])*
        $name:ident) => {
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
            ::borsh::BorshSerialize,
        )]
        #[serde(transparent)]
        $(#[$meta])*
        pub struct $name(#[serde(with = "::ootle_serde::hex")] ::tari_common_types::types::FixedHash);

        impl $name {
            /// Represents a zero/null hash.
            pub const fn zero() -> Self {
                Self(::tari_common_types::types::FixedHash::zero())
            }

            pub fn new<T: Into<::tari_common_types::types::FixedHash>>(hash: T) -> Self {
                Self(hash.into())
            }

            pub const fn hash(&self) -> &::tari_common_types::types::FixedHash {
                &self.0
            }

            pub fn as_bytes(&self) -> &[u8] {
                self.0.as_slice()
            }

            pub fn is_zero(&self) -> bool {
                self.0.iter().all(|b| *b == 0)
            }

            pub const fn into_array(self) -> [u8; 32] {
                self.0.into_array()
            }

            pub const fn byte_size() -> usize {
                ::tari_common_types::types::FixedHash::byte_size()
            }
        }

        impl AsRef<[u8]> for $name {
            fn as_ref(&self) -> &[u8] {
                self.0.as_slice()
            }
        }

        impl From<::tari_common_types::types::FixedHash> for $name {
            fn from(value: ::tari_common_types::types::FixedHash) -> Self {
                Self(value)
            }
        }

        impl From<[u8; ::tari_common_types::types::FixedHash::byte_size()]> for $name {
            fn from(value: [u8; ::tari_common_types::types::FixedHash::byte_size()]) -> Self {
                Self(value.into())
            }
        }

        impl TryFrom<Vec<u8>> for $name {
            type Error = tari_common_types::types::FixedHashSizeError;

            fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
                value.as_slice().try_into()
            }
        }

        impl TryFrom<&[u8]> for $name {
            type Error = tari_common_types::types::FixedHashSizeError;

            fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
                ::tari_common_types::types::FixedHash::try_from(value).map(Self)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Display::fmt(&self.0, f)
            }
        }

        impl AsRef<$name> for $name {
            fn as_ref(&self) -> &$name {
                self
            }
        }
    };
}
