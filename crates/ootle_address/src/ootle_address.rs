//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display, io, str::FromStr};

use bech32::{Bech32m, Hrp};
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_engine_types::{ConvertFromByteType, FromByteType, ToByteType};
use tari_ootle_common_types::{Network, NetworkParseError};
use tari_template_lib_types::{crypto::RistrettoPublicKeyBytes, InvalidByteLengthError};

use crate::hrp::{hrp_from_network, network_from_hrp};

#[derive(Debug, Clone, PartialEq, Eq)]
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
            .map_err(|_| OotleAddressError::InvalidPublicKey)
    }

    fn to_view_only_key_ristretto(&self) -> Result<RistrettoPublicKey, OotleAddressError> {
        self.view_only_key
            .try_from_byte_type()
            .map_err(|_| OotleAddressError::InvalidPublicKey)
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
        let account_key = RistrettoPublicKeyBytes::from_bytes(&buf)?;
        reader.read_exact(&mut buf)?;
        let view_only_key = RistrettoPublicKeyBytes::from_bytes(&buf)?;
        Ok(OotleAddress::new(network, view_only_key, account_key))
    }

    pub fn encode_to_writer<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&[self.network as u8])?;
        self.encode_keys_to_writer(writer)?;
        Ok(())
    }

    fn encode_keys_to_writer<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(self.account_key.as_bytes())?;
        writer.write_all(self.view_only_key.as_bytes())?;
        Ok(())
    }

    pub fn encode_bech32_to_fmt<W: fmt::Write>(&self, f: &mut W) -> Result<(), OotleAddressError> {
        let hrp = hrp_from_network(self.network);
        const KL: usize = RistrettoPublicKeyBytes::length();
        let mut buf = [0u8; KL * 2];
        self.encode_keys_to_writer(&mut buf.as_mut_slice())?;
        bech32::encode_lower_to_fmt::<Bech32m, _>(f, hrp, &buf)?;
        Ok(())
    }

    pub fn decode_bech32(s: &str) -> Result<Self, OotleAddressError> {
        const KL: usize = RistrettoPublicKeyBytes::length();
        let (hrp, data) = bech32::decode(s)?;
        let network = network_from_hrp(&hrp).ok_or(OotleAddressError::UnrecognisedHrp { hrp })?;
        if data.len() != KL * 2 {
            return Err(OotleAddressError::AddressIncorrectLength {
                expected: RistrettoPublicKeyBytes::length() * 2,
                actual: data.len(),
            });
        }
        let account_key = RistrettoPublicKeyBytes::from_bytes(&data[..KL])?;
        let view_only_key = RistrettoPublicKeyBytes::from_bytes(&data[KL..KL * 2])?;
        Ok(OotleAddress::new(network, view_only_key, account_key))
    }

    pub fn to_bech32_string(&self) -> String {
        let mut s = String::with_capacity(119);
        self.encode_bech32_to_fmt(&mut s).unwrap();
        s
    }
}

impl Display for OotleAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.encode_bech32_to_fmt(f).map_err(|_| fmt::Error)
    }
}

impl FromStr for OotleAddress {
    type Err = OotleAddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::decode_bech32(s)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OotleAddressError {
    #[error("Bech32 decode error: {0}")]
    Bech32DecodeError(#[from] bech32::DecodeError),
    #[error("Bech32 encode error: {0}")]
    Bech32EncodeError(#[from] bech32::EncodeError),
    #[error("Unrecognised HRP: {hrp}")]
    UnrecognisedHrp { hrp: Hrp },
    #[error(transparent)]
    InvalidNetwork(#[from] NetworkParseError),
    #[error("Invalid address bytes: {0}")]
    InvalidAddressBytes(#[from] InvalidByteLengthError),
    #[error("Address has incorrect length: expected {expected} bytes, got {actual} bytes")]
    AddressIncorrectLength { expected: usize, actual: usize },
    #[error("Invalid public key")]
    InvalidPublicKey,
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
                // Serialize as string
                let s = self.to_bech32_string();
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
                // Deserialize from string
                let s = String::deserialize(deserializer)?;
                OotleAddress::decode_bech32(&s).map_err(D::Error::custom)
            } else {
                // Deserialize from bytes
                let bytes: Cow<'_, [u8]> = Deserialize::deserialize(deserializer)?;
                OotleAddress::from_bytes(&bytes).map_err(D::Error::custom)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    fn it_encodes_to_bech32() {
        let addr = sample(1);
        let mut encoded = String::new();
        addr.encode_bech32_to_fmt(&mut encoded).unwrap();
        assert_eq!(encoded.len(), 118);
        assert!(encoded.starts_with(&hrp_from_network(Network::LocalNet).to_string()));
    }

    #[test]
    fn it_decodes_from_bech32() {
        let addr = sample(2);
        let mut encoded = String::new();
        addr.encode_bech32_to_fmt(&mut encoded).unwrap();
        let decoded = OotleAddress::decode_bech32(&encoded).unwrap();
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
