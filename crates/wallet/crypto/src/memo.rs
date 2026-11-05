//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{cmp, fmt::Display, io};

use tari_template_lib_types::{MaxBytes, MaxString, hex::write_hex_fmt};

/// These are selected to be out of range of the Minotari memo field tags
/// See:
/// https://github.com/tari-project/tari/blob/221d715e2447e6ca33e2ebcba11e915d24edac15/base_layer/transaction_components/src/transaction_components/memo_field.rs
#[repr(u8)]
enum MemoTag {
    U256 = 0x01,
    Message = 0x10,
    Bytes = 0x11,
    DestinationTagAndBytes = 0x12,
    SenderAddress = 0x13,
}

impl MemoTag {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::U256),
            0x10 => Some(Self::Message),
            0x11 => Some(Self::Bytes),
            0x12 => Some(Self::DestinationTagAndBytes),
            0x13 => Some(Self::SenderAddress),
            _ => None,
        }
    }
}

const MAX_BYTES_LENGTH: usize = 253;

/// Length of the key portion of a sender address memo body: account public key (32) + view public key (32).
const SENDER_ADDRESS_KEYS_LENGTH: usize = 64;
/// Maximum destination tag length carried by a sender address memo. Matches
/// `tari_ootle_address::DestinationTag::MAX_LEN`.
const SENDER_ADDRESS_MAX_DESTINATION_TAG_LENGTH: usize = 64;
/// Maximum encoded sender address body: keys (64) + destination tag length prefix (1) + destination tag (<= 64).
const SENDER_ADDRESS_MAX_LENGTH: usize = SENDER_ADDRESS_KEYS_LENGTH + 1 + SENDER_ADDRESS_MAX_DESTINATION_TAG_LENGTH;

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Memo {
    /// Fixed length 32-byte. This is supported for compatibility with Minotari U256 memos and allows
    /// Ootle wallets to understand them. Note that only the custom encoding is compatible with Minotari,
    /// not the serde encoding.
    U256(#[cfg_attr(feature = "ts", ts(type = "string"))] MaxBytes<32>),
    /// UTF-8 encoded string message
    Message(#[cfg_attr(feature = "ts", ts(type = "string"))] MaxString<MAX_BYTES_LENGTH>),
    /// Arbitrary bytes
    Bytes(#[cfg_attr(feature = "ts", ts(type = "string"))] MaxBytes<MAX_BYTES_LENGTH>),
    /// Destination tag and bytes delimited by a single byte length prefix.
    /// Length-delimited format: [destination_tag_len][destination_tag][arb bytes (typically utf-8 message)]
    DestinationTagAndBytes(#[cfg_attr(feature = "ts", ts(type = "string"))] MaxBytes<MAX_BYTES_LENGTH>),
    /// The sender's Ootle address, allowing the recipient to identify and save the sender as a contact.
    /// Encoded like an `OotleAddress` without the leading network byte (the network is implied by the wallet that
    /// decrypts the memo): account public key (32) ++ view public key (32) ++ destination tag length (1) ++ destination
    /// tag (0-64). The destination tag is optional.
    SenderAddress(#[cfg_attr(feature = "ts", ts(type = "string"))] MaxBytes<SENDER_ADDRESS_MAX_LENGTH>),
}

impl Memo {
    /// EncryptedData memo size (255) - 2 (memo tag + length (u8))
    pub const MAX_BYTES_LENGTH: usize = MAX_BYTES_LENGTH;

    pub fn new_u256(value: [u8; 32]) -> Self {
        let b = MaxBytes::new_checked(value).expect("32 < MAX_BYTES_LENGTH");
        Self::U256(b)
    }

    /// Create a new `Memo::SenderAddress` from the sender's account public key, view public key and an optional
    /// destination tag. Returns `None` if `destination_tag` exceeds the maximum destination tag length (64 bytes).
    pub fn new_sender_address(account_key: [u8; 32], view_key: [u8; 32], destination_tag: &[u8]) -> Option<Self> {
        if destination_tag.len() > SENDER_ADDRESS_MAX_DESTINATION_TAG_LENGTH {
            return None;
        }
        let mut body = Vec::with_capacity(SENDER_ADDRESS_KEYS_LENGTH + 1 + destination_tag.len());
        body.extend_from_slice(&account_key);
        body.extend_from_slice(&view_key);
        body.push(destination_tag.len() as u8); // <= 64, fits in u8
        body.extend_from_slice(destination_tag);
        let b = MaxBytes::new_checked(body)?;
        Some(Self::SenderAddress(b))
    }

    pub fn new_message(s: impl Into<Box<str>>) -> Option<Self> {
        let s = s.into();
        let s = MaxString::new_checked(s)?;
        Some(Self::Message(s))
    }

    pub fn new_bytes(b: impl Into<Box<[u8]>>) -> Option<Self> {
        let b = b.into();
        let b = MaxBytes::new_checked(b)?;
        Some(Self::Bytes(b))
    }

    pub fn new_destination_tag_and_message<P: AsRef<[u8]>>(destination_tag: P, msg: &str) -> Option<Self> {
        Self::new_destination_tag_and_bytes(destination_tag, msg)
    }

    /// Create a new `Memo::DestinationTagAndBytes` from a destination tag and message. If the combined length exceeds
    /// the maximum allowed length, the message is truncated to fit.
    pub fn new_destination_tag_and_message_truncate<P: AsRef<[u8]>>(destination_tag: P, msg: &str) -> Option<Self> {
        Self::new_destination_tag_and_bytes_truncate(destination_tag, msg.as_bytes())
    }

    /// Create a new `Memo::DestinationTagAndBytes` from a destination tag and bytes. If the combined length exceeds
    /// the maximum allowed length, the memo bytes are truncated to fit.
    ///
    /// Returns `None` if the destination tag alone exceeds the maximum allowed length (i.e. it cannot fit even with
    /// an empty message).
    pub fn new_destination_tag_and_bytes_truncate<P: AsRef<[u8]>, B: AsRef<[u8]>>(
        destination_tag: P,
        msg_bytes: B,
    ) -> Option<Self> {
        let tag = destination_tag.as_ref();
        let available_len = (Self::MAX_BYTES_LENGTH - 1).checked_sub(tag.len())?; // -1 for length prefix byte
        let mb = msg_bytes.as_ref();
        let mb = if mb.len() > available_len {
            &mb[..available_len]
        } else {
            mb
        };
        Self::new_destination_tag_and_bytes(destination_tag, mb)
    }

    pub fn new_destination_tag_and_bytes<P: AsRef<[u8]>, B: AsRef<[u8]>>(
        destination_tag: P,
        msg_bytes: B,
    ) -> Option<Self> {
        let tag = destination_tag.as_ref();
        let mb = msg_bytes.as_ref();
        // - 1 for length prefix byte
        if mb.len() + tag.len() > Self::MAX_BYTES_LENGTH - 1 {
            return None;
        }
        let mut combined = Vec::with_capacity(mb.len() + tag.len());
        combined.push(u8::try_from(tag.len()).expect("new_destination_tag_and_bytes: length checked"));
        combined.extend_from_slice(tag);
        combined.extend_from_slice(mb);
        let b = MaxBytes::new_checked(combined)?;
        Some(Self::DestinationTagAndBytes(b))
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Message(s) => s.len(),
            Self::Bytes(b) => b.len(),
            Self::DestinationTagAndBytes(b) => b.len(),
            Self::U256(b) => b.len(),
            Self::SenderAddress(b) => b.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Attempt to interpret the memo as a UTF-8 string, regardless of its variant.
    /// Returns `None` if the memo cannot be represented as a UTF-8 string.
    pub fn as_utf8_str(&self) -> Option<&str> {
        match self {
            Self::Message(s) => Some(s),
            Self::Bytes(b) => str::from_utf8(b.as_ref()).ok(),
            Self::DestinationTagAndBytes(body) => {
                let (_, msg_bytes) = split_len_prefixed(body)?;
                str::from_utf8(msg_bytes).ok()
            },
            Self::U256(_) | Self::SenderAddress(_) => None,
        }
    }

    pub fn as_memo_message(&self) -> Option<&str> {
        match self {
            Self::Message(s) => Some(s),
            Self::Bytes(_) | Self::DestinationTagAndBytes(_) | Self::U256(_) | Self::SenderAddress(_) => None,
        }
    }

    pub fn as_memo_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(b) => Some(b.as_ref()),
            Self::Message(_) | Self::DestinationTagAndBytes(_) | Self::U256(_) | Self::SenderAddress(_) => None,
        }
    }

    pub fn as_destination_tag(&self) -> Option<&[u8]> {
        match self {
            Self::U256(_) | Self::Message(_) | Self::Bytes(_) | Self::SenderAddress(_) => None,
            Self::DestinationTagAndBytes(body) => split_len_prefixed(body).map(|(destination_tag, _)| destination_tag),
        }
    }

    pub fn as_destination_tag_and_message(&self) -> Option<(&[u8], &str)> {
        self.as_destination_tag_and_bytes()
            .and_then(|(destination_tag, msg_bytes)| {
                let msg = str::from_utf8(msg_bytes).ok()?;
                Some((destination_tag, msg))
            })
    }

    pub fn as_destination_tag_and_bytes(&self) -> Option<(&[u8], &[u8])> {
        match self {
            Self::U256(_) | Self::Message(_) | Self::Bytes(_) | Self::SenderAddress(_) => None,
            Self::DestinationTagAndBytes(body) => {
                let (destination_tag, msg_bytes) = split_len_prefixed(body)?;
                Some((destination_tag, msg_bytes))
            },
        }
    }

    pub fn as_u256_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::U256(b) => Some(b.as_ref()),
            Self::Message(_) | Self::Bytes(_) | Self::DestinationTagAndBytes(_) | Self::SenderAddress(_) => None,
        }
    }

    /// Returns the sender's `(account_public_key, view_public_key, destination_tag)` byte slices if this is a
    /// `SenderAddress` memo. The account and view keys are 32 bytes each; `destination_tag` is 0-64 bytes (empty when
    /// no destination tag was attached).
    pub fn as_sender_address(&self) -> Option<(&[u8], &[u8], &[u8])> {
        match self {
            Self::SenderAddress(b) => split_sender_address(b.as_ref()),
            Self::U256(_) | Self::Message(_) | Self::Bytes(_) | Self::DestinationTagAndBytes(_) => None,
        }
    }

    // We implement a custom encoding (that **almost** matches borsh). Because we are very byte constrained and since we
    // know that the max length is 253 bytes we can use a single byte for the length. Borsh uses a u32 for length (3
    // extra bytes of overhead).

    pub fn encode_into<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        let len = self.len();
        let len = u8::try_from(len).expect("len <= MAX_BYTES_LENGTH <= 255");
        match self {
            Self::U256(b) => {
                writer.write_all(&[MemoTag::U256 as u8])?;
                writer.write_all(b.as_ref())?;
            },
            Self::Message(s) => {
                writer.write_all(&[MemoTag::Message as u8])?;
                writer.write_all(&[len])?;
                writer.write_all(s.as_bytes())?;
            },
            Self::Bytes(b) => {
                writer.write_all(&[MemoTag::Bytes as u8])?;
                writer.write_all(&[len])?;
                writer.write_all(b.as_ref())?;
            },
            Self::DestinationTagAndBytes(b) => {
                writer.write_all(&[MemoTag::DestinationTagAndBytes as u8])?;
                writer.write_all(&[len])?;
                writer.write_all(b.as_ref())?;
            },
            Self::SenderAddress(b) => {
                // The body is self-delimiting (fixed-size keys + internal destination tag length prefix), so we do not
                // write an outer length prefix.
                writer.write_all(&[MemoTag::SenderAddress as u8])?;
                writer.write_all(b.as_ref())?;
            },
        }
        Ok(())
    }

    pub fn decode_from<R: io::Read>(reader: &mut R) -> io::Result<Self> {
        let mut tag = [0u8; 1];
        reader.read_exact(&mut tag)?;
        let tag = MemoTag::from_u8(tag[0]);

        let Some(tag) = tag else {
            // Fallback for future versions, preserve unknown memos as bytes (up to the max size)
            let mut buf = vec![0u8; Self::MAX_BYTES_LENGTH];
            let bytes_read = read_until_len_or_eof(&mut buf, reader, Self::MAX_BYTES_LENGTH)?;
            buf.truncate(bytes_read);
            return Ok(Self::new_bytes(buf).expect("length checked"));
        };

        match tag {
            MemoTag::U256 => {
                let mut arr = [0u8; 32];
                reader.read_exact(&mut arr)?;
                Ok(Self::new_u256(arr))
            },
            MemoTag::SenderAddress => {
                // Read the fixed-size keys and the destination tag length prefix, then the destination tag itself.
                let mut head = [0u8; SENDER_ADDRESS_KEYS_LENGTH + 1];
                reader.read_exact(&mut head)?;
                let destination_tag_len = head[SENDER_ADDRESS_KEYS_LENGTH] as usize;
                if destination_tag_len > SENDER_ADDRESS_MAX_DESTINATION_TAG_LENGTH {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "Sender address destination tag length {} exceeds maximum {}",
                            destination_tag_len, SENDER_ADDRESS_MAX_DESTINATION_TAG_LENGTH
                        ),
                    ));
                }
                let mut body = Vec::with_capacity(head.len() + destination_tag_len);
                body.extend_from_slice(&head);
                let mut destination_tag = vec![0u8; destination_tag_len];
                reader.read_exact(&mut destination_tag)?;
                body.extend_from_slice(&destination_tag);
                let b = MaxBytes::new_checked(body).expect("length checked (body <= SENDER_ADDRESS_MAX_LENGTH)");
                Ok(Self::SenderAddress(b))
            },
            MemoTag::Message => {
                let len = read_len_prefix(reader)?;
                let mut buf = vec![0u8; len];
                reader.read_exact(&mut buf)?;
                let s =
                    String::from_utf8(buf).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid UTF-8"))?;
                Ok(Self::new_message(s).expect("length checked (buf.len() <= MAX_BYTES_LENGTH)"))
            },
            MemoTag::Bytes => {
                let len = read_len_prefix(reader)?;
                let mut buf = vec![0u8; len];
                reader.read_exact(&mut buf)?;
                Ok(Self::new_bytes(buf).expect("length checked (buf.len() <= MAX_BYTES_LENGTH)"))
            },
            MemoTag::DestinationTagAndBytes => {
                let len = read_len_prefix(reader)?;
                let mut buf = vec![0u8; len];
                reader.read_exact(&mut buf)?;
                // Validate the length-delimited encoding
                let _ = split_len_prefixed(&buf).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "Invalid MessageAndDestinationTag encoding")
                })?;
                let bytes = MaxBytes::new_checked(buf).expect("length checked (buf.len() <= MAX_BYTES_LENGTH)");
                Ok(Self::DestinationTagAndBytes(bytes))
            },
        }
    }
}

impl Display for Memo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::U256(b) => {
                write!(f, "U256(")?;
                write_hex_fmt(f, b)?;
                write!(f, ")")
            },
            Self::Message(s) => write!(f, "Message({})", s),
            Self::Bytes(b) => {
                write!(f, "Bytes(")?;
                write_hex_fmt(f, b)?;
                write!(f, ")")
            },
            Self::DestinationTagAndBytes(b) => {
                let Some((destination_tag, msg_bytes)) = split_len_prefixed(b) else {
                    return write!(f, "DestinationTagAndBytes(<invalid encoding>)");
                };
                write!(
                    f,
                    "DestinationTagAndBytes(destination_tag: {}, bytes: ",
                    std::str::from_utf8(destination_tag).unwrap_or("<not utf-8>"),
                )?;
                write_hex_fmt(f, msg_bytes)?;
                write!(f, ")")
            },
            Self::SenderAddress(b) => {
                let Some((account_key, view_key, destination_tag)) = split_sender_address(b) else {
                    return write!(f, "SenderAddress(<invalid encoding>)");
                };
                write!(f, "SenderAddress(account_key: ")?;
                write_hex_fmt(f, account_key)?;
                write!(f, ", view_key: ")?;
                write_hex_fmt(f, view_key)?;
                if !destination_tag.is_empty() {
                    write!(f, ", destination_tag: ")?;
                    write_hex_fmt(f, destination_tag)?;
                }
                write!(f, ")")
            },
        }
    }
}

fn read_until_len_or_eof<R: io::Read>(mut buf: &mut [u8], reader: &mut R, max_len: usize) -> io::Result<usize> {
    let mut bytes_read = 0;
    while !buf.is_empty() {
        match reader.read(buf) {
            Ok(0) => break,
            Ok(n) => {
                // Subtraction is safe because bytes_read <= max_len
                let bytes_to_take = cmp::min(n, max_len - bytes_read);
                bytes_read += bytes_to_take;
                // Copied from std::io::Read::read_exact but added the following to break if we reach max_len
                if bytes_read == max_len {
                    break;
                }

                buf = &mut buf[bytes_to_take..];
            },
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {},
            Err(e) => return Err(e),
        }
    }

    Ok(bytes_read)
}

fn read_len_prefix<R: io::Read>(reader: &mut R) -> io::Result<usize> {
    let mut len_buf = [0u8; 1];
    reader.read_exact(&mut len_buf)?;
    let len = len_buf[0] as usize;
    if len > MAX_BYTES_LENGTH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Memo length specifier {} exceeds maximum {}", len, MAX_BYTES_LENGTH),
        ));
    }
    Ok(len)
}

/// Splits a `SenderAddress` body into `(account_public_key, view_public_key, destination_tag)`. The body layout is
/// `account(32) ++ view(32) ++ destination_tag_len(1) ++ destination_tag`. Returns `None` if the body is malformed.
fn split_sender_address(body: &[u8]) -> Option<(&[u8], &[u8], &[u8])> {
    if body.len() < SENDER_ADDRESS_KEYS_LENGTH + 1 {
        return None;
    }
    let account_key = &body[..32];
    let view_key = &body[32..SENDER_ADDRESS_KEYS_LENGTH];
    let destination_tag_len = body[SENDER_ADDRESS_KEYS_LENGTH] as usize;
    let destination_tag =
        body.get(SENDER_ADDRESS_KEYS_LENGTH + 1..SENDER_ADDRESS_KEYS_LENGTH + 1 + destination_tag_len)?;
    Some((account_key, view_key, destination_tag))
}

fn split_len_prefixed(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    if bytes.is_empty() {
        return None;
    }

    // Len prefixed
    let len = bytes[0] as usize;
    if len > bytes.len() - 1 {
        return None;
    }

    bytes[1..].split_at_checked(len)
}

#[cfg(test)]
mod tests {
    use tari_template_lib_types::EncryptedData;

    use super::*;

    #[test]
    fn it_allows_empty_data() {
        let memo = Memo::new_message("").unwrap();
        assert_eq!(memo.len(), 0);
        // Encode/decode empty
        let mut buf = Vec::new();
        memo.encode_into(&mut buf).unwrap();
        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(memo, decoded);

        let memo = Memo::new_bytes(vec![]).unwrap();
        assert_eq!(memo.len(), 0);
        // Encode/decode empty
        let mut buf = Vec::new();
        memo.encode_into(&mut buf).unwrap();
        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(memo, decoded);

        let memo = Memo::new_destination_tag_and_message([], "").unwrap();
        assert_eq!(memo.len(), 1); // 1 byte for the destination_tag length
        // Encode/decode empty
        let mut buf = Vec::new();
        memo.encode_into(&mut buf).unwrap();
        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(memo, decoded);
    }

    #[test]
    fn it_returns_none_if_max_bytes_len_exceeded() {
        let bytes = vec![0u8; Memo::MAX_BYTES_LENGTH + 1];
        let memo = Memo::new_bytes(bytes);
        assert!(memo.is_none());
    }

    #[test]
    fn it_encodes_and_decodes() {
        // U256
        let original = Memo::new_u256([1u8; 32]);
        let mut buf = Vec::new();
        original.encode_into(&mut buf).unwrap();
        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(original, decoded);

        let original = Memo::new_message("Hello, world!").unwrap();
        let mut buf = Vec::new();
        original.encode_into(&mut buf).unwrap();
        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(original, decoded);

        let original = Memo::new_bytes(vec![1, 2, 3, 4, 5]).unwrap();
        let mut buf = Vec::new();
        original.encode_into(&mut buf).unwrap();
        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(original, decoded);

        // Empty memo
        let original = Memo::new_message("").unwrap();
        let mut buf = Vec::new();
        original.encode_into(&mut buf).unwrap();
        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(original, decoded);

        // Max size memo
        let bytes = vec![0u8; Memo::MAX_BYTES_LENGTH];
        let original = Memo::new_bytes(bytes).unwrap();
        let mut buf = Vec::new();
        original.encode_into(&mut buf).unwrap();
        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(original, decoded);

        // DestinationTag and Message
        let destination_tag = [1, 2, 3];
        let msg = "Payment for services";
        let original = Memo::new_destination_tag_and_message(destination_tag, msg).unwrap();
        let mut buf = Vec::new();
        original.encode_into(&mut buf).unwrap();
        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(original, decoded);
        assert_eq!(original.as_memo_message(), decoded.as_memo_message());
    }

    #[test]
    fn it_fails_to_decode_if_length_is_too_big() {
        // THis is valid encoding but the length exceeds the max allowed length
        let encoded = [
            vec![MemoTag::Bytes as u8, (Memo::MAX_BYTES_LENGTH + 1) as u8],
            vec![1u8; Memo::MAX_BYTES_LENGTH + 1],
        ]
        .concat();
        let _err = Memo::decode_from(&mut encoded.as_slice()).unwrap_err();
    }

    #[test]
    fn it_fails_to_decode_if_length_mismatches() {
        // This is invalid encoding as the length (10) exceeds the actual data length (5)
        let encoded = [vec![MemoTag::Message as u8, 10u8], vec![1u8; 5]].concat();
        let _err = Memo::decode_from(&mut encoded.as_slice()).unwrap_err();
    }

    #[test]
    fn it_falls_back_to_bytes_with_unknown_variants() {
        let encoded = [vec![0u8, 5], vec![1u8; 5]].concat();
        let decoded = Memo::decode_from(&mut encoded.as_slice()).unwrap();
        // Includes the length byte in the bytes since some future unknown variant may not be length prefixed
        assert_eq!(decoded, Memo::new_bytes([5, 1, 1, 1, 1, 1]).unwrap());
    }

    #[test]
    fn it_encodes_to_max_bytes() {
        let bytes = vec![0u8; Memo::MAX_BYTES_LENGTH];
        let memo = Memo::new_bytes(bytes).unwrap();
        let mut buf = Vec::new();
        memo.encode_into(&mut buf).unwrap();
        // We want the encoded memo to fit in the max size of an EncryptedData payload (i.e 255 bytes)
        assert!(buf.len() <= EncryptedData::max_size() - EncryptedData::min_size());
    }

    #[test]
    fn it_encodes_and_decodes_sender_address_without_destination_tag() {
        let account_key = [7u8; 32];
        let view_key = [9u8; 32];
        let original = Memo::new_sender_address(account_key, view_key, &[]).unwrap();

        let mut buf = Vec::new();
        original.encode_into(&mut buf).unwrap();
        // tag (1) + account_key (32) + view_key (32) + destination_tag_len (1)
        assert_eq!(buf.len(), 1 + SENDER_ADDRESS_KEYS_LENGTH + 1);
        assert_eq!(buf[0], MemoTag::SenderAddress as u8);

        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(original, decoded);

        let (decoded_account, decoded_view, decoded_destination_tag) = decoded.as_sender_address().unwrap();
        assert_eq!(decoded_account, &account_key);
        assert_eq!(decoded_view, &view_key);
        assert!(decoded_destination_tag.is_empty());
    }

    #[test]
    fn it_encodes_and_decodes_sender_address_with_destination_tag() {
        let account_key = [7u8; 32];
        let view_key = [9u8; 32];
        let destination_tag = vec![3u8; SENDER_ADDRESS_MAX_DESTINATION_TAG_LENGTH];
        let original = Memo::new_sender_address(account_key, view_key, &destination_tag).unwrap();

        let mut buf = Vec::new();
        original.encode_into(&mut buf).unwrap();
        // tag (1) + keys (64) + destination_tag_len (1) + destination_tag (64)
        assert_eq!(buf.len(), 1 + SENDER_ADDRESS_KEYS_LENGTH + 1 + destination_tag.len());

        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(original, decoded);

        let (decoded_account, decoded_view, decoded_destination_tag) = decoded.as_sender_address().unwrap();
        assert_eq!(decoded_account, &account_key);
        assert_eq!(decoded_view, &view_key);
        assert_eq!(decoded_destination_tag, destination_tag.as_slice());
    }

    #[test]
    fn it_rejects_sender_address_destination_tag_that_is_too_long() {
        let too_long = vec![1u8; SENDER_ADDRESS_MAX_DESTINATION_TAG_LENGTH + 1];
        assert!(Memo::new_sender_address([0u8; 32], [0u8; 32], &too_long).is_none());
    }

    #[test]
    fn it_decodes_sender_address_ignoring_trailing_padding() {
        let original = Memo::new_sender_address([1u8; 32], [2u8; 32], &[42, 43, 44]).unwrap();
        let mut buf = Vec::new();
        original.encode_into(&mut buf).unwrap();
        // Simulate the zero padding that the EncryptedData encoder appends after the memo.
        buf.extend_from_slice(&[0u8; 63]);
        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn it_fails_to_decode_with_an_invalid_destination_tag_length() {
        // DestinationTag length (10) exceeds actual data length (5)
        let encoded = [vec![MemoTag::DestinationTagAndBytes as u8, 10u8], vec![1u8; 5]].concat();
        let _err = Memo::decode_from(&mut encoded.as_slice()).unwrap_err();
    }
}
