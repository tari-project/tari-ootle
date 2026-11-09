//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display, io, str::FromStr};

use bech32::{Bech32m, Hrp};
use ootle_byte_type::{ConvertFromByteType, FromByteType, ToByteType};
use ootle_network::{Network, NetworkParseError};
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_template_lib_types::{InvalidByteLengthError, crypto::RistrettoPublicKeyBytes};

use crate::{
    destination_tag::DestinationTag,
    hrp::{hrp_from_network, network_from_hrp},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, type = "string"))]
pub struct OotleAddress {
    network: Network,
    view_only_key: RistrettoPublicKeyBytes,
    account_key: RistrettoPublicKeyBytes,
    destination_tag: Option<DestinationTag>,
}

impl OotleAddress {
    /// Minimum byte length of the encoded address: network (1) + view_only_key (32) + account_key (32) +
    /// destination_tag len (1)
    const MIN_BYTE_LENGTH: usize = 1 + 32 + 32 + 1;

    pub fn new(network: Network, view_only_key: RistrettoPublicKeyBytes, account_key: RistrettoPublicKeyBytes) -> Self {
        Self {
            network,
            view_only_key,
            account_key,
            destination_tag: None,
        }
    }

    /// Adds a destination tag to the address.
    pub fn with_destination_tag(mut self, destination_tag: DestinationTag) -> Self {
        self.destination_tag = Some(destination_tag);
        self
    }

    /// Removes the destination tag from the address.
    pub fn without_destination_tag(mut self) -> Self {
        self.destination_tag = None;
        self
    }

    pub fn validate(&self) -> Result<(), OotleAddressError> {
        self.to_account_key_ristretto()?;
        self.to_view_only_key_ristretto()?;
        Ok(())
    }

    pub const fn byte_length(&self) -> usize {
        Self::MIN_BYTE_LENGTH + self.destination_tag_len()
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

    pub fn destination_tag(&self) -> Option<&DestinationTag> {
        self.destination_tag.as_ref()
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

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::MIN_BYTE_LENGTH);
        self.encode_to_writer(&mut buf).unwrap();
        buf
    }

    pub fn from_bytes(mut bytes: &[u8]) -> Result<Self, OotleAddressError> {
        let reader = &mut bytes;
        let address = Self::decode_from_reader(reader)?;
        if !reader.is_empty() {
            return Err(OotleAddressError::BytesRemaining {
                remaining: reader.len(),
            });
        }
        Ok(address)
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

        let mut buf = [0u8; 1];
        reader.read_exact(&mut buf)?;
        let destination_tag_len = buf[0] as usize;
        if destination_tag_len > DestinationTag::MAX_LEN {
            return Err(OotleAddressError::InvalidAddressBytes(InvalidByteLengthError::new(
                destination_tag_len,
                DestinationTag::MAX_LEN,
            )));
        }

        let destination_tag = if destination_tag_len > 0 {
            let mut tag_buf = vec![0u8; destination_tag_len];
            reader
                .read_exact(&mut tag_buf)
                .map_err(|e| OotleAddressError::InvalidDestinationTagLengthSpecifier {
                    given_len: destination_tag_len,
                    source: e,
                })?;
            Some(
                DestinationTag::from_bytes(&tag_buf).expect("decode_from_reader: destination_tag_len checked and read"),
            )
        } else {
            None
        };

        Ok(Self {
            network,
            view_only_key,
            account_key,
            destination_tag,
        })
    }

    pub fn encode_to_writer<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&[self.network as u8])?;
        self.encode_keys_to_writer(writer)?;
        // Write the length of the destination tag as a u8.
        let destination_tag_len = u8::try_from(self.destination_tag_len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invariant violation: destination tag length exceeds 255 (u8::MAX)",
            )
        })?;
        writer.write_all(&[destination_tag_len])?;

        self.encode_destination_tag_to_writer(writer)?;
        Ok(())
    }

    fn encode_keys_to_writer<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(self.account_key.as_bytes())?;
        writer.write_all(self.view_only_key.as_bytes())?;
        Ok(())
    }

    fn encode_destination_tag_to_writer<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        if let Some(destination_tag) = &self.destination_tag {
            writer.write_all(destination_tag.as_ref())?;
        }
        Ok(())
    }

    const fn encoded_bech32_payload_len(&self) -> usize {
        // const_option_ops and const_option_basic are unstable, so we'll implement these operations manually
        let kl = RistrettoPublicKeyBytes::length();
        match self.destination_tag {
            Some(ref tag) => kl * 2 + tag.len(),
            None => kl * 2,
        }
    }

    pub fn encode_bech32_to_fmt<W: fmt::Write>(&self, f: &mut W) -> Result<(), OotleAddressError> {
        let hrp = hrp_from_network(self.network);
        let len = self.encoded_bech32_payload_len();
        let mut buf = vec![0u8; len];
        let writer = &mut buf.as_mut_slice();
        self.encode_keys_to_writer(writer)?;
        self.encode_destination_tag_to_writer(writer)?;
        bech32::encode_lower_to_fmt::<Bech32m, _>(f, hrp, &buf)?;
        Ok(())
    }

    pub fn decode_bech32(s: &str) -> Result<Self, OotleAddressError> {
        const KL: usize = RistrettoPublicKeyBytes::length();
        let (hrp, data) = bech32::decode(s)?;
        let network = network_from_hrp(&hrp).ok_or(OotleAddressError::UnrecognisedHrp { hrp })?;
        if data.len() < KL * 2 {
            return Err(OotleAddressError::AddressLengthTooShort {
                minimum: KL * 2,
                actual: data.len(),
            });
        }
        let account_key =
            RistrettoPublicKeyBytes::from_bytes(data.get(..KL).expect("decode_bech32: len checked (spend key)"))?;
        let view_only_key =
            RistrettoPublicKeyBytes::from_bytes(data.get(KL..KL * 2).expect("decode_bech32: len checked (view key)"))?;

        let mut address = OotleAddress::new(network, view_only_key, account_key);
        if data.len() > KL * 2 {
            let destination_tag = DestinationTag::from_bytes(
                data.get(KL * 2..)
                    .expect("decode_bech32: len checked (destination tag)"),
            )
            .ok_or(OotleAddressError::AddressLengthTooLong {
                maximum: KL * 2 + DestinationTag::MAX_LEN,
                actual: data.len(),
            })?;
            address = address.with_destination_tag(destination_tag);
        }

        Ok(address)
    }

    const fn destination_tag_len(&self) -> usize {
        // const_option_ops and const_option_basic are unstable, so we'll implement these operations manually
        match self.destination_tag {
            Some(ref tag) => tag.len(),
            None => 0,
        }
    }

    pub fn to_bech32_string(&self) -> String {
        let tag_len = self.destination_tag_len();
        let mut s = String::with_capacity(119 + tag_len);
        self.encode_bech32_to_fmt(&mut s).unwrap();
        s
    }

    pub fn into_ristretto_address(self) -> Result<RistrettoOotleAddress, OotleAddressError> {
        Ok(RistrettoOotleAddress {
            network: self.network,
            view_only_key: self.to_view_only_key_ristretto()?,
            account_key: self.to_account_key_ristretto()?,
            destination_tag: self.destination_tag,
        })
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
    #[error("Address length is too short: minimum {minimum} bytes, got {actual} bytes")]
    AddressLengthTooShort { minimum: usize, actual: usize },
    #[error("Address length is too long: maximum {maximum} bytes, got {actual} bytes")]
    AddressLengthTooLong { maximum: usize, actual: usize },
    #[error("{remaining} unexpected bytes remaining after decoding address")]
    BytesRemaining { remaining: usize },
    #[error("Invalid destination tag length specifier: given length {given_len}, source error: {source}")]
    InvalidDestinationTagLengthSpecifier { given_len: usize, source: io::Error },
    #[error("Invalid public key")]
    InvalidPublicKey,
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

#[cfg(feature = "serde")]
mod serde_impl {
    use std::borrow::Cow;

    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeError};

    use super::*;

    impl Serialize for OotleAddress {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            if serializer.is_human_readable() {
                // Serialize as string
                let s = self.to_bech32_string();
                serializer.serialize_str(&s)
            } else {
                // Serialize as bytes
                let bytes = self.to_bytes();
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
    pub destination_tag: Option<DestinationTag>,
}

impl RistrettoOotleAddress {
    pub fn new(network: Network, view_only_key: RistrettoPublicKey, account_key: RistrettoPublicKey) -> Self {
        Self {
            network,
            view_only_key,
            account_key,
            destination_tag: None,
        }
    }

    pub fn with_destination_tag(mut self, destination_tag: DestinationTag) -> Self {
        self.destination_tag = Some(destination_tag);
        self
    }

    pub fn network(&self) -> Network {
        self.network
    }

    pub fn view_only_key(&self) -> &RistrettoPublicKey {
        &self.view_only_key
    }

    pub fn account_key(&self) -> &RistrettoPublicKey {
        &self.account_key
    }

    pub fn destination_tag(&self) -> Option<&DestinationTag> {
        self.destination_tag.as_ref()
    }

    pub fn into_byte_address(self) -> OotleAddress {
        OotleAddress {
            network: self.network,
            view_only_key: self.view_only_key.to_byte_type(),
            account_key: self.account_key.to_byte_type(),
            destination_tag: self.destination_tag,
        }
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
            destination_tag: bytes.destination_tag.clone(),
        })
    }
}

impl ToByteType for RistrettoOotleAddress {
    type ByteType = OotleAddress;

    fn to_byte_type(&self) -> Self::ByteType {
        OotleAddress {
            network: self.network,
            view_only_key: self.view_only_key.to_byte_type(),
            account_key: self.account_key.to_byte_type(),
            destination_tag: self.destination_tag.clone(),
        }
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
        assert_eq!(addr.destination_tag(), None);
    }

    #[test]
    fn it_encodes_and_decodes_from_bytes() {
        let addr = sample(3);
        let bytes = addr.to_bytes();
        let decoded = OotleAddress::from_bytes(&bytes).unwrap();
        assert_eq!(addr.network(), decoded.network());
        assert_eq!(addr.view_only_key(), decoded.view_only_key());
        assert_eq!(addr.account_public_key(), decoded.account_public_key());
        assert_eq!(addr.destination_tag(), None);
    }

    #[test]
    fn it_parses_from_str() {
        let addr = sample(4);
        let s = addr.to_string();
        let parsed: OotleAddress = s.parse().unwrap();
        assert_eq!(addr.network(), parsed.network());
        assert_eq!(addr.view_only_key(), parsed.view_only_key());
        assert_eq!(addr.account_public_key(), parsed.account_public_key());
        assert_eq!(addr.destination_tag(), None);
    }

    mod with_destination_tag {
        use std::iter;

        use super::*;

        #[test]
        fn it_encodes_and_decodes_with_destination_tag() {
            let destination_tag = DestinationTag::new_checked(vec![1; DestinationTag::MAX_LEN]).unwrap();
            let addr = sample(10).with_destination_tag(destination_tag.clone());
            let bytes = addr.to_bytes();
            let decoded = OotleAddress::from_bytes(&bytes).unwrap();
            assert_eq!(addr.network(), decoded.network());
            assert_eq!(addr.view_only_key(), decoded.view_only_key());
            assert_eq!(addr.account_public_key(), decoded.account_public_key());
            assert_eq!(addr.destination_tag(), Some(&destination_tag));
        }

        #[test]
        fn it_errors_if_destination_tag_length_is_inaccurate() {
            let mut bytes = sample(11).to_bytes();
            // Encode destination_tag length to an incorrect value
            let invalid_destination_tag_len = DestinationTag::MAX_LEN as u8;
            // Say we have MAX_LEN bytes but we have 0
            let destination_tag_len_index = 1 + 32 + 32; // network (1) + account_key (32) + view_only_key (32)
            bytes[destination_tag_len_index] = invalid_destination_tag_len;
            let result = OotleAddress::from_bytes(&bytes);
            assert!(
                matches!(
                    result,
                    Err(OotleAddressError::InvalidDestinationTagLengthSpecifier {
                        given_len: DestinationTag::MAX_LEN,
                        ..
                    })
                ),
                "{:?}",
                result
            );
            // Say we have 10 bytes but we have 12
            let mut bytes = sample(11).to_bytes();
            bytes[destination_tag_len_index] = 10;
            bytes.extend(iter::repeat_n(12, 12)); // add dummy destination_tag bytes
            let result = OotleAddress::from_bytes(&bytes);
            assert!(
                matches!(result, Err(OotleAddressError::BytesRemaining { remaining: 2 })),
                "Got: {:?}",
                result
            );

            // Say we have 12 bytes but we have 10
            let mut bytes = sample(11).to_bytes();
            bytes[destination_tag_len_index] = 12;
            bytes.extend(iter::repeat_n(12, 10)); // add dummy destination_tag bytes
            let result = OotleAddress::from_bytes(&bytes);
            assert!(
                matches!(
                    result,
                    Err(OotleAddressError::InvalidDestinationTagLengthSpecifier { given_len: 12, .. })
                ),
                "Got: {:?}",
                result
            );
        }

        #[test]
        fn it_errors_if_destination_tag_is_too_large() {
            let mut bytes = sample(11).to_bytes();
            // Encode destination_tag length to an incorrect value
            let invalid_destination_tag_len = DestinationTag::MAX_LEN as u8 + 1;
            let destination_tag_len_index = 1 + 32 + 32; // network (1) + account_key (32) + view_only_key (32)
            bytes[destination_tag_len_index] = invalid_destination_tag_len;
            bytes.extend(iter::repeat_n(12, invalid_destination_tag_len as usize)); // add dummy destination_tag bytes
            let result = OotleAddress::from_bytes(&bytes);
            assert!(
                matches!(result, Err(OotleAddressError::InvalidAddressBytes(_))),
                "{:?}",
                result
            );
        }
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
            assert_eq!(addr.destination_tag(), None);
        }

        #[test]
        fn it_serializes_to_bytes() {
            let addr = sample(6);
            let bytes = tari_bor::serde_codec::to_vec(&addr).unwrap();
            let deserialized: OotleAddress = tari_bor::serde_codec::from_slice(&bytes).unwrap();
            assert_eq!(addr.network(), deserialized.network());
            assert_eq!(addr.view_only_key(), deserialized.view_only_key());
            assert_eq!(addr.account_public_key(), deserialized.account_public_key());
            assert_eq!(addr.destination_tag(), None);
        }

        mod with_destination_tag {
            use super::*;

            #[test]
            fn it_serializes_to_json_with_destination_tag() {
                let destination_tag = DestinationTag::new_checked(vec![10; DestinationTag::MAX_LEN]).unwrap();
                let addr = sample(7).with_destination_tag(destination_tag.clone());
                let json = serde_json::to_string(&addr).unwrap();
                let deserialized: OotleAddress = serde_json::from_str(&json).unwrap();
                assert_eq!(addr.network(), deserialized.network());
                assert_eq!(addr.view_only_key(), deserialized.view_only_key());
                assert_eq!(addr.account_public_key(), deserialized.account_public_key());
                assert_eq!(addr.destination_tag(), Some(&destination_tag));
            }

            #[test]
            fn it_serializes_to_bytes_with_destination_tag() {
                let destination_tag = DestinationTag::new_checked(vec![40, 50, 60, 70]).unwrap();
                let addr = sample(8).with_destination_tag(destination_tag.clone());
                let bytes = tari_bor::serde_codec::to_vec(&addr).unwrap();
                let deserialized: OotleAddress = tari_bor::serde_codec::from_slice(&bytes).unwrap();
                assert_eq!(addr.network(), deserialized.network());
                assert_eq!(addr.view_only_key(), deserialized.view_only_key());
                assert_eq!(addr.account_public_key(), deserialized.account_public_key());
                assert_eq!(addr.destination_tag(), Some(&destination_tag));
            }
        }
    }
}
