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

use serde::{Deserialize, Serialize};
use tari_bor::BorTag;
use tari_template_abi::rust::{
    collections::{BTreeMap, btree_map},
    fmt,
    fmt::Display,
    prelude::*,
    str::FromStr,
};

use super::BinaryTag;

const TAG: u64 = BinaryTag::Metadata as u64;

/// A collection of user-defined data used to describe other types, for example, non-fungible tokens or events
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Metadata(BorTag<BTreeMap<String, String>, TAG>);

impl Metadata {
    pub const fn new() -> Self {
        Self(BorTag::new(BTreeMap::new()))
    }

    pub fn insert<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) -> &mut Self {
        let key = key.into();
        let value = value.into();
        self.0.insert(key, value);
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|s| s.as_str())
    }

    pub fn get_or_insert<K: Into<String>, V: Into<String>>(&mut self, key: K, default: V) -> &str {
        let key = key.into();
        self.0.entry(key).or_insert_with(|| default.into())
    }

    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.0.remove(key)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    pub fn merge(&mut self, other: Metadata) -> &mut Self {
        self.0.extend(other.0.into_inner());
        self
    }
}

impl FromStr for Metadata {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let pairs = s.split(',').map(|pair| {
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| "Invalid key=value pair".to_string())?;
            Ok::<_, String>((key.trim().to_string(), value.trim().to_string()))
        });
        let mut map = BTreeMap::new();
        for pair in pairs {
            let (key, value) = pair?;
            map.insert(key, value);
        }
        Ok(Self(BorTag::new(map)))
    }
}

impl From<()> for Metadata {
    fn from(_: ()) -> Self {
        Self::new()
    }
}

impl From<BTreeMap<String, String>> for Metadata {
    fn from(value: BTreeMap<String, String>) -> Self {
        Self(BorTag::new(value))
    }
}

impl<K: Into<String>, V: Into<String>, const N: usize> From<[(K, V); N]> for Metadata {
    fn from(value: [(K, V); N]) -> Self {
        Self(BorTag::new(BTreeMap::from(value.map(|(k, v)| (k.into(), v.into())))))
    }
}

impl IntoIterator for Metadata {
    type IntoIter = btree_map::IntoIter<String, String>;
    type Item = (String, String);

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_inner().into_iter()
    }
}

impl<K: ToString, V: Into<String>> FromIterator<(K, V)> for Metadata {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self(BorTag::new(
            iter.into_iter().map(|(k, v)| (k.to_string(), v.into())).collect(),
        ))
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
            write!(f, "{} = {}", key, value)?;
            if i < self.0.len() - 1 {
                write!(f, ", ")?;
            }
        }
        Ok(())
    }
}

/// Creates a metadata object
///
/// # Example
///
/// ```rust
/// # use tari_template_lib_types::metadata;
/// metadata!(
///   "name" => "My NFT",
///   "description" => "This is my first NFT",
///   "image" => "https://example.com/my-nft.png"
/// );
/// ```
#[macro_export]
macro_rules! metadata {
    ($($key:expr => $value:expr),* $(,)?) => {
        {
            let mut metadata = $crate::Metadata::new();
            $(
                metadata.insert($key, $value);
            )*
            metadata
        }
    };
    () => {
        $crate::models::Metadata::new()
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_macro() {
        let i = 123;
        let metadata = metadata!(
            "name" => "My NFT",
            "description" => "This is my first NFT",
            "image" => "https://example.com/my-nft.png",
            "index" => i.to_string()
        );

        assert_eq!(metadata.get("name"), Some("My NFT"));
        assert_eq!(metadata.get("description"), Some("This is my first NFT"));
        assert_eq!(metadata.get("image"), Some("https://example.com/my-nft.png"));
        assert_eq!(metadata.get("index"), Some("123"));
    }

    #[test]
    fn to_str_from_str() {
        let original_metadata = metadata!(
            "name" => "My NFT",
            "description" => "This is my first NFT",
            "image" => "https://example.com/my-nft.png"
        );

        let metadata_str = original_metadata.to_string();
        let parsed_metadata = metadata_str.parse::<Metadata>().unwrap();

        assert_eq!(original_metadata, parsed_metadata);
    }
}
