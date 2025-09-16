//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display, io, str::FromStr};

use bs58::encode::EncodeTarget;
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_engine_types::{ConvertFromByteType, FromByteType, ToByteType};
use tari_ootle_common_types::{Network, NetworkParseError};
use tari_template_lib::{prelude::RistrettoPublicKeyBytes, types::InvalidByteLengthError};
use tari_utilities::ByteArrayError;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, type = "string"))]
pub struct OotleAddress {
    network: Network,
    view_only_key: RistrettoPublicKeyBytes,
    account_key: RistrettoPublicKeyBytes,
}

impl OotleAddress {
    /// Byte length of the encoded address: network (1) + view_only_key (32) + account_key (32)
    pub const BYTE_LENGTH: usize = 1 + 32 + 32;

    pub fn new(network: Network, view_only_key: RistrettoPublicKeyBytes, account_key: RistrettoPublicKeyBytes) -> Self {
        Self {
            network,
            view_only_key,
            account_key,
        }
    }

    pub fn validate(&self) -> Result<(), OotleAddressError> {
        self.to_account_key_ristretto()?;
        self.to_view_only_key_ristretto()?;
        Ok(())
    }

    pub fn network(&self) -> Network {
        self.network
    }

    pub fn view_only_key(&self) -> &RistrettoPublicKeyBytes {
        &self.view_only_key
    }

    pub fn account_public_key(&self) -> &RistrettoPublicKeyBytes {
        &self.account_key
    }

    fn to_account_key_ristretto(&self) -> Result<RistrettoPublicKey, OotleAddressError> {
        self.account_key
            .try_from_byte_type()
            .map_err(OotleAddressError::InvalidPublicKey)
    }

    fn to_view_only_key_ristretto(&self) -> Result<RistrettoPublicKey, OotleAddressError> {
        self.view_only_key
            .try_from_byte_type()
            .map_err(OotleAddressError::InvalidPublicKey)
    }

    pub fn to_byte_array(&self) -> [u8; Self::BYTE_LENGTH] {
        let mut buf = [0u8; Self::BYTE_LENGTH];
        self.encode_to_writer(&mut buf.as_mut_slice())
            .expect("Buffer with sufficient capacity");
        buf
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::BYTE_LENGTH);
        self.encode_to_writer(&mut buf).unwrap();
        buf
    }

    pub fn from_bytes(mut bytes: &[u8]) -> Result<Self, OotleAddressError> {
        Self::decode_from_reader(&mut bytes)
    }

    pub fn decode_from_reader<R: io::Read>(reader: &mut R) -> Result<Self, OotleAddressError> {
        let mut buf = [0u8; 1];
        reader.read_exact(&mut buf)?;
        let network = Network::try_from(buf[0])?;
        let mut buf = [0u8; 32];
        reader.read_exact(&mut buf)?;
        let view_only_key = RistrettoPublicKeyBytes::from_bytes(&buf)?;
        reader.read_exact(&mut buf)?;
        let account_key = RistrettoPublicKeyBytes::from_bytes(&buf)?;
        Ok(OotleAddress::new(network, view_only_key, account_key))
    }

    pub fn encode_to_writer<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&[self.network as u8])?;
        writer.write_all(self.view_only_key.as_bytes())?;
        writer.write_all(self.account_key.as_bytes())?;
        Ok(())
    }

    pub fn encode_as_base58<E: EncodeTarget>(&self, target: E) -> Result<(), OotleAddressError> {
        let mut buf = [0u8; Self::BYTE_LENGTH];
        self.encode_to_writer(&mut buf.as_mut_slice())?;
        bs58::encode(buf).onto(target)?;
        Ok(())
    }

    pub fn decode_base58(s: &str) -> Result<Self, OotleAddressError> {
        let mut buf = [0u8; Self::BYTE_LENGTH];
        bs58::decode(s).onto(&mut buf)?;
        let network = Network::try_from(buf[0])?;
        let view_only_key = RistrettoPublicKeyBytes::from_bytes(&buf[1..33])?;
        let account_key = RistrettoPublicKeyBytes::from_bytes(&buf[33..65])?;
        Ok(OotleAddress::new(network, view_only_key, account_key))
    }

    pub fn to_base58(&self) -> String {
        const MAX_ENCODED_LENGTH: usize = 89; // Base58 encoded length of 65 bytes is at most 89 characters
        let mut encoded = String::with_capacity(MAX_ENCODED_LENGTH);
        self.encode_as_base58(&mut encoded)
            .expect("String with sufficient capacity");
        encoded
    }
}

impl Display for OotleAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_base58())
    }
}

impl FromStr for OotleAddress {
    type Err = OotleAddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::decode_base58(s)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OotleAddressError {
    #[error("Base58 decode error: {0}")]
    Base58DecodeError(#[from] bs58::decode::Error),
    #[error("Base58 encode error: {0}")]
    Base58EncodeError(#[from] bs58::encode::Error),
    #[error(transparent)]
    InvalidNetwork(#[from] NetworkParseError),
    #[error("Invalid address bytes: {0}")]
    InvalidAddressBytes(#[from] InvalidByteLengthError),
    #[error("Invalid public key: {0}")]
    InvalidPublicKey(ByteArrayError),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

#[cfg(feature = "serde")]
mod serde_impl {
    use std::borrow::Cow;

    use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize, Serializer};

    use super::*;

    impl Serialize for OotleAddress {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            if serializer.is_human_readable() {
                // Serialize as base58 string
                let s = self.to_base58();
                serializer.serialize_str(&s)
            } else {
                // Serialize as bytes
                let bytes = self.to_byte_array();
                serializer.serialize_bytes(&bytes)
            }
        }
    }

    impl<'de> Deserialize<'de> for OotleAddress {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            if deserializer.is_human_readable() {
                // Deserialize from base58 string
                let s = String::deserialize(deserializer)?;
                OotleAddress::decode_base58(&s).map_err(D::Error::custom)
            } else {
                // Deserialize from bytes
                let bytes: Cow<'_, [u8]> = Deserialize::deserialize(deserializer)?;
                OotleAddress::from_bytes(&bytes).map_err(D::Error::custom)
            }
        }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RistrettoOotleAddress {
    pub network: Network,
    pub view_only_key: RistrettoPublicKey,
    pub account_key: RistrettoPublicKey,
}

impl RistrettoOotleAddress {
    pub fn network(&self) -> Network {
        self.network
    }

    pub fn view_only_key(&self) -> &RistrettoPublicKey {
        &self.view_only_key
    }

    pub fn account_key(&self) -> &RistrettoPublicKey {
        &self.account_key
    }
}

impl ConvertFromByteType<OotleAddress> for RistrettoOotleAddress {
    type Error = OotleAddressError;

    fn convert_from_byte_type(bytes: &OotleAddress) -> Result<Self, Self::Error>
    where Self: Sized {
        Ok(Self {
            network: bytes.network,
            view_only_key: bytes.to_view_only_key_ristretto()?,
            account_key: bytes.to_account_key_ristretto()?,
        })
    }
}

impl ToByteType for RistrettoOotleAddress {
    type ByteType = OotleAddress;

    fn to_byte_type(&self) -> Self::ByteType {
        OotleAddress::new(
            self.network,
            self.view_only_key.to_byte_type(),
            self.account_key.to_byte_type(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(seed: u8) -> OotleAddress {
        let view_only_key = RistrettoPublicKeyBytes::from([seed; 32]);
        let account_key = RistrettoPublicKeyBytes::from([seed + 1; 32]);
        OotleAddress::new(Network::LocalNet, view_only_key, account_key)
    }

    #[test]
    fn it_encodes_to_base_58() {
        let addr = sample(1);
        let mut encoded = String::new();
        addr.encode_as_base58(&mut encoded).unwrap();
        assert_eq!(encoded.len(), 89);
        assert!(encoded.starts_with('2')); // LocalNet addresses start with '2' in Base58
    }

    #[test]
    fn it_decodes_from_base_58() {
        let addr = sample(2);
        let mut encoded = String::new();
        addr.encode_as_base58(&mut encoded).unwrap();
        let decoded = OotleAddress::decode_base58(&encoded).unwrap();
        assert_eq!(addr.network(), decoded.network());
        assert_eq!(addr.view_only_key(), decoded.view_only_key());
        assert_eq!(addr.account_public_key(), decoded.account_public_key());
    }

    #[test]
    fn it_encodes_and_decodes_from_bytes() {
        let addr = sample(3);
        let bytes = addr.to_bytes();
        let decoded = OotleAddress::from_bytes(&bytes).unwrap();
        assert_eq!(addr.network(), decoded.network());
        assert_eq!(addr.view_only_key(), decoded.view_only_key());
        assert_eq!(addr.account_public_key(), decoded.account_public_key());
    }

    #[test]
    fn it_parses_from_str() {
        let addr = sample(4);
        let s = addr.to_string();
        let parsed: OotleAddress = s.parse().unwrap();
        assert_eq!(addr.network(), parsed.network());
        assert_eq!(addr.view_only_key(), parsed.view_only_key());
        assert_eq!(addr.account_public_key(), parsed.account_public_key());
    }

    #[cfg(feature = "serde")]
    mod serde_tests {

        use super::*;

        #[test]
        fn it_serializes_to_json() {
            let addr = sample(5);
            let json = serde_json::to_string(&addr).unwrap();
            let deserialized: OotleAddress = serde_json::from_str(&json).unwrap();
            assert_eq!(addr.network(), deserialized.network());
            assert_eq!(addr.view_only_key(), deserialized.view_only_key());
            assert_eq!(addr.account_public_key(), deserialized.account_public_key());
        }

        #[test]
        fn it_serializes_to_bytes() {
            let addr = sample(6);
            let bytes = bincode::serde::encode_to_vec(&addr, bincode::config::standard()).unwrap();
            let (deserialized, _): (OotleAddress, _) =
                bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
            assert_eq!(addr.network(), deserialized.network());
            assert_eq!(addr.view_only_key(), deserialized.view_only_key());
            assert_eq!(addr.account_public_key(), deserialized.account_public_key());
        }
    }
}
