//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use minicbor::{CborLen, Decode, Encode};
use tari_bor::{BorError, BorTag, RawCbor};
use tari_template_abi::rust::{
    collections::{BTreeMap, btree_map},
    fmt,
    fmt::Display,
    format,
    prelude::*,
    str::FromStr,
};

use super::BinaryTag;

const TAG: u64 = BinaryTag::Metadata as u64;

/// A collection of user-defined data used to describe other types, for example, non-fungible tokens or events.
///
/// Values are arbitrary CBOR, each held as the exact encoding its producer wrote, so a value that goes
/// into a hashed substate comes back out byte for byte. Read one with [`Self::get_as`] for a typed
/// value or [`Self::get_str`] when it is known to be text.
#[derive(Clone, Debug, PartialEq, Encode, Decode, CborLen)]
#[cbor(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Metadata(
    #[cfg_attr(feature = "ts", ts(type = "Record<string, any>"))] BorTag<BTreeMap<String, RawCbor>, TAG>,
);

impl Metadata {
    pub const fn new() -> Self {
        Self(BorTag::new(BTreeMap::new()))
    }

    /// Encodes `value` and stores it under `key`, replacing any previous entry.
    ///
    /// # Panics
    ///
    /// Panics if `value` cannot be encoded. Use [`Self::try_insert`] where that must be an error.
    pub fn insert<K: Into<String>, V: Encode<()> + ?Sized>(&mut self, key: K, value: &V) -> &mut Self {
        self.try_insert(key, value)
            .expect("Metadata::insert: value cannot be encoded as CBOR")
    }

    /// Encodes `value` and stores it under `key`, replacing any previous entry.
    pub fn try_insert<K: Into<String>, V: Encode<()> + ?Sized>(
        &mut self,
        key: K,
        value: &V,
    ) -> Result<&mut Self, BorError> {
        let key = key.into();
        let value = RawCbor::from_encodable(value)?;
        self.0.insert(key, value);
        Ok(self)
    }

    /// Stores an already-encoded value under `key`, replacing any previous entry.
    pub fn insert_raw<K: Into<String>>(&mut self, key: K, value: RawCbor) -> &mut Self {
        self.0.insert(key.into(), value);
        self
    }

    pub fn get(&self, key: &str) -> Option<&RawCbor> {
        self.0.get(key)
    }

    /// Decodes the value at `key` as `T`.
    ///
    /// `Ok(None)` when there is no such key; `Err` when there is one and it does not decode as `T`.
    pub fn get_as<T: for<'b> Decode<'b, ()>>(&self, key: &str) -> Result<Option<T>, BorError> {
        self.0.get(key).map(RawCbor::decode).transpose()
    }

    /// The value at `key` when it is a CBOR text string, otherwise `None`.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        // A `RawCbor` is exactly one item, so the borrowed decode consumes all of it.
        minicbor::decode::<&str>(self.0.get(key)?.as_bytes()).ok()
    }

    pub fn remove(&mut self, key: &str) -> Option<RawCbor> {
        self.0.remove(key)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> btree_map::Iter<'_, String, RawCbor> {
        self.0.iter()
    }

    pub fn merge(&mut self, other: Metadata) -> &mut Self {
        self.0.extend(other.0.into_inner());
        self
    }
}

impl FromStr for Metadata {
    type Err = String;

    /// Parses `key=value` pairs, each value stored as a CBOR text string.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut metadata = Self::new();
        if s.trim().is_empty() {
            return Ok(metadata);
        }
        for pair in s.split(',') {
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| "Invalid key=value pair".to_string())?;
            let key = key.trim();
            if key.is_empty() {
                return Err("Metadata key cannot be empty".to_string());
            }
            metadata
                .try_insert(key, value.trim())
                .map_err(|e| format!("Failed to encode metadata value: {}", e))?;
        }
        Ok(metadata)
    }
}

impl From<()> for Metadata {
    fn from(_: ()) -> Self {
        Self::new()
    }
}

impl From<BTreeMap<String, RawCbor>> for Metadata {
    fn from(value: BTreeMap<String, RawCbor>) -> Self {
        Self(BorTag::new(value))
    }
}

/// # Panics
///
/// Panics if a value cannot be encoded.
impl<K: Into<String>, V: Encode<()>, const N: usize> From<[(K, V); N]> for Metadata {
    fn from(value: [(K, V); N]) -> Self {
        value.into_iter().collect()
    }
}

/// # Panics
///
/// Panics if a value cannot be encoded.
impl<K: Into<String>, V: Encode<()>> FromIterator<(K, V)> for Metadata {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut metadata = Self::new();
        for (key, value) in iter {
            metadata.insert(key, &value);
        }
        metadata
    }
}

impl IntoIterator for Metadata {
    type IntoIter = btree_map::IntoIter<String, RawCbor>;
    type Item = (String, RawCbor);

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_inner().into_iter()
    }
}

impl<'a> IntoIterator for &'a Metadata {
    type IntoIter = btree_map::Iter<'a, String, RawCbor>;
    type Item = (&'a String, &'a RawCbor);

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl Default for Metadata {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for Metadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, (key, value)) in self.0.iter().enumerate() {
            match value.to_value() {
                Ok(value) => write!(f, "{} = {:?}", key, value)?,
                Err(_) => write!(f, "{} = <{} bytes of cbor>", key, value.as_bytes().len())?,
            }
            if i < self.0.len() - 1 {
                write!(f, ", ")?;
            }
        }
        Ok(())
    }
}

/// Creates a metadata object.
///
/// Values are encoded as CBOR, so any encodable type may be used.
///
/// # Example
///
/// ```rust
/// # use tari_template_lib_types::metadata;
/// metadata!(
///   "name" => "My NFT",
///   "description" => "This is my first NFT",
///   "image" => "https://example.com/my-nft.png",
///   "index" => 123u32,
/// );
/// ```
///
/// # Panics
///
/// Panics if a value cannot be encoded as CBOR.
#[macro_export]
macro_rules! metadata {
    ($($key:expr => $value:expr),* $(,)?) => {
        {
            let mut metadata = $crate::Metadata::new();
            $(
                metadata.insert($key, &$value);
            )*
            metadata
        }
    };
    () => {
        $crate::Metadata::new()
    };
}

/// Creates a metadata object like [`metadata!`], returning an encoding failure instead of panicking.
///
/// # Example
///
/// ```rust
/// # use tari_template_lib_types::try_metadata;
/// let metadata = try_metadata!("name" => "My NFT", "index" => 123u32)?;
/// # Ok::<_, tari_template_lib_types::BorError>(())
/// ```
#[macro_export]
macro_rules! try_metadata {
    () => {
        ::core::result::Result::<$crate::Metadata, $crate::BorError>::Ok($crate::Metadata::new())
    };
    ($($key:expr => $value:expr),+ $(,)?) => {
        'try_metadata: {
            let mut metadata = $crate::Metadata::new();
            $(
                if let ::core::result::Result::Err(e) = metadata.try_insert($key, &$value) {
                    break 'try_metadata ::core::result::Result::Err(e);
                }
            )*
            ::core::result::Result::<$crate::Metadata, $crate::BorError>::Ok(metadata)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_macro() {
        let i = 123u32;
        let metadata = metadata!(
            "name" => "My NFT",
            "description" => "This is my first NFT",
            "image" => "https://example.com/my-nft.png",
            "index" => i
        );

        assert_eq!(metadata.get_str("name"), Some("My NFT"));
        assert_eq!(metadata.get_str("description"), Some("This is my first NFT"));
        assert_eq!(metadata.get_str("image"), Some("https://example.com/my-nft.png"));
        assert_eq!(metadata.get_as::<u32>("index").unwrap().unwrap(), 123);
    }

    #[test]
    fn a_value_is_stored_as_the_bytes_the_producer_wrote() {
        let mut metadata = Metadata::new();
        metadata.insert("index", &123u32);

        assert_eq!(
            metadata.get("index").unwrap().as_bytes(),
            tari_bor::encode(&123u32).unwrap().as_slice()
        );
    }

    #[test]
    fn try_metadata_matches_metadata() {
        assert_eq!(
            try_metadata!("name" => "My NFT", "index" => 123u32).unwrap(),
            metadata!("name" => "My NFT", "index" => 123u32)
        );
        assert_eq!(try_metadata!().unwrap(), Metadata::new());
    }

    #[test]
    fn a_non_text_value_has_no_str_form() {
        let metadata = metadata!("index" => 123u32);
        assert_eq!(metadata.get_str("index"), None);
    }

    #[test]
    fn to_str_from_str() {
        let original_metadata = metadata!(
            "name" => "My NFT",
            "description" => "This is my first NFT",
            "image" => "https://example.com/my-nft.png"
        );

        let parsed_metadata = "name=My NFT,description=This is my first NFT,image=https://example.com/my-nft.png"
            .parse::<Metadata>()
            .unwrap();

        assert_eq!(original_metadata, parsed_metadata);
        assert!("=val".parse::<Metadata>().is_err());
        assert!("a=1, =2".parse::<Metadata>().is_err());
    }
}
