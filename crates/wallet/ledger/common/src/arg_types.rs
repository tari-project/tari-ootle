//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Borsh-encoded APDU request/response bodies and the framing of the streamed `SignTransaction`
//! exchange.

use borsh::{BorshDeserialize, BorshSerialize};

use crate::OotleStatusWord;

/// Key-derivation branch for [`GetPublicKeyRequest`] and [`SignTransactionHeader`].
///
/// Mirrors `tari_ootle_wallet_sdk::models::KeyBranch` variant-for-variant so host and device
/// derive the same keys.
#[repr(u64)]
#[derive(Clone, Copy, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
pub enum KeyType {
    /// The account key branch, used for deriving account keys.
    Account = 0x00,
    /// The transaction key branch, used to sign transactions that do not need to be signed with the account key.
    Transaction = 0x01,
    /// The Elgamal encryption view key branch, used to derive a view key for resources with "viewable balance"
    /// enabled.
    ElgamalEncryptionViewKey = 0x02,
    /// The stealth mask branch, used to derive masks for stealth addresses.
    StealthMask = 0x03,
    /// The confidential mask branch, used to derive masks for confidential transactions.
    ConfidentialMask = 0x04,
    /// Used to generate nonces that need to be recreated later, e.g. to derive the DH secret for claim burn
    Nonce = 0x05,
    /// Branch used to derive view-only keys. This key is used to derive an encryption key for wallet recovery. But
    /// does not allow spending.
    ViewOnlyKey = 0x06,
}

impl KeyType {
    /// The branch's borsh discriminant, used as the derivation-path component.
    pub fn as_u64(&self) -> u64 {
        *self as u64
    }
}

/// The tweak that turns a derived account key into a stealth one-time key: the spent UTXO's sender
/// public nonce `R`, and the network the resulting key is bound to.
///
/// Both travel together because neither is meaningful alone — the tweak is
/// `c = H_stealth_owner(network, v·R)` (`v` the view key at the same derivation path), so the same
/// nonce yields a different key on each network.
#[derive(BorshSerialize, BorshDeserialize)]
pub struct StealthTweak {
    pub network: u8,
    pub public_nonce: [u8; 32],
}

/// Body of a `GetPublicKey` request: the derivation path of the key to return.
///
/// When `stealth` is set the device returns the stealth one-time key `(c + k)·G` for that tweak
/// rather than the derived key `k·G` itself. This is the key a stealth input signs with, so it must
/// be resolvable before any signature binds to it.
#[derive(BorshSerialize, BorshDeserialize)]
pub struct GetPublicKeyRequest {
    pub account: u64,
    pub index: u64,
    pub key_type: KeyType,
    pub stealth: Option<StealthTweak>,
}

/// Response to `GetPublicKey`: the compressed Ristretto public key for the requested path.
#[derive(BorshSerialize, BorshDeserialize)]
pub struct GetPublicKeyResponse {
    pub public_key: [u8; 32],
}

/// Which signature procedure the device should perform for a `SignTransaction` stream.
///
/// Selects both the key/usage semantics on the host (`add signer` vs `seal sign`) and the
/// domain-separation label the device hashes under (`"Signature"` vs `"SealSignature"`).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
pub enum SignMode {
    /// Authorization signature added by an extra signer; binds the seal signer's public key.
    AddSigner = 0x00,
    /// Final seal signature by the transaction originator; binds the prior signatures.
    Seal = 0x01,
}

/// Canonical field order for an authorization signature. Matches
/// `tari_ootle_transaction::TransactionSignature::signing_preimage_v1`.
pub const ADD_SIGNER_SEQ: &[SigningField] = &[
    SigningField::SchemaVersion,
    SigningField::SealSigner,
    SigningField::Network,
    SigningField::FeeInstructions,
    SigningField::Instructions,
    SigningField::Inputs,
    SigningField::MinEpoch,
    SigningField::MaxEpoch,
    SigningField::MainSigner,
    SigningField::DryRun,
    SigningField::TransactionNonce,
    SigningField::BlobHashes,
];

/// Canonical field order for a seal signature. Matches
/// `tari_ootle_transaction::TransactionSealSignature::signing_preimage_v1`.
pub const SEAL_SEQ: &[SigningField] = &[
    SigningField::SchemaVersion,
    SigningField::Network,
    SigningField::FeeInstructions,
    SigningField::Instructions,
    SigningField::Inputs,
    SigningField::MinEpoch,
    SigningField::MaxEpoch,
    SigningField::MainSigner,
    SigningField::DryRun,
    SigningField::TransactionNonce,
    SigningField::BlobHashes,
    SigningField::Signatures,
];

impl SignMode {
    /// Expected sequence of [`SigningField`]s for this signing mode.
    pub const fn expected_fields(self) -> &'static [SigningField] {
        match self {
            SignMode::AddSigner => ADD_SIGNER_SEQ,
            SignMode::Seal => SEAL_SEQ,
        }
    }
}

/// `SignTransaction` APDU frame kind, carried in `P2`.
///
/// A signing exchange is: one `Header`, then one `Segment` (possibly split across several APDUs)
/// per canonical preimage field in order, then one `Finalize` which triggers the user review and
/// returns the signature.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Header = 0x00,
    Segment = 0x01,
    Finalize = 0x02,
}

impl TryFrom<u8> for FrameKind {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(FrameKind::Header),
            0x01 => Ok(FrameKind::Segment),
            0x02 => Ok(FrameKind::Finalize),
            _ => Err(()),
        }
    }
}

/// `Segment` frames carry the field tag in the low 7 bits of `P1`; this bit (the high bit) marks
/// the last APDU chunk of that field's bytes.
pub const SEGMENT_LAST_CHUNK: u8 = 0x80;

/// A field of the canonical transaction signing preimage, in chain order.
///
/// MUST stay numerically in lock-step with `tari_ootle_transaction::PreimageField` — the host
/// streams `field as u8` in `P1` and the device reconstructs the exact byte sequence chained into
/// the message digest. The lock-step is asserted by a test in the ledger client crate.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigningField {
    SchemaVersion = 0,
    SealSigner = 1,
    Network = 2,
    FeeInstructions = 3,
    Instructions = 4,
    Inputs = 5,
    MinEpoch = 6,
    MaxEpoch = 7,
    MainSigner = 8,
    DryRun = 9,
    TransactionNonce = 10,
    BlobHashes = 11,
    Signatures = 12,
}

impl SigningField {
    /// The field tag carried in the low 7 bits of `P1` on `Segment` frames.
    pub fn as_tag(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for SigningField {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use SigningField::*;
        match value {
            0 => Ok(SchemaVersion),
            1 => Ok(SealSigner),
            2 => Ok(Network),
            3 => Ok(FeeInstructions),
            4 => Ok(Instructions),
            5 => Ok(Inputs),
            6 => Ok(MinEpoch),
            7 => Ok(MaxEpoch),
            8 => Ok(MainSigner),
            9 => Ok(DryRun),
            10 => Ok(TransactionNonce),
            11 => Ok(BlobHashes),
            12 => Ok(Signatures),
            _ => Err(()),
        }
    }
}

/// Body of the `Header` frame that opens a `SignTransaction` stream.
///
/// Identifies the signing key (same derivation params as [`GetPublicKeyRequest`]) and the
/// procedure. The transaction fields themselves follow as `Segment` frames.
///
/// `stealth_public_nonce` is set for confidential (stealth) transfers: when present, the device
/// signs with the stealth-derived key `c + k` (`c = H_stealth_owner(network, v·R)`, `v` the view key
/// at the same derivation path, `R` the spent UTXO's sender public nonce) instead of the raw account
/// key. It is a key-derivation parameter only — not part of the signed message — so the message
/// recipe is identical to the public path.
#[derive(BorshSerialize, BorshDeserialize)]
pub struct SignTransactionHeader {
    pub account: u64,
    pub index: u64,
    pub key_type: KeyType,
    pub mode: SignMode,
    pub stealth_public_nonce: Option<[u8; 32]>,
}

/// Response returned on the `Finalize` frame once the user approves.
///
/// `signature` is the standard 64-byte sr25519 encoding: `R.compress()(32) || s(32)` with the
/// marker bit `0x80` set in the last byte.
#[derive(BorshSerialize, BorshDeserialize)]
pub struct SignTransactionResponse {
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
}

/// Summary fields extracted from the fixed-offset prefix of the streamed preimage.
///
/// `SchemaVersion`, `SealSigner` (when present) and `Network` have exact byte lengths, so
/// `network` and the leading `u32` count of `FeeInstructions` sit at fixed offsets in the
/// unframed preimage.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TxDisplay {
    pub network: u8,
    pub fee_instruction_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActiveSegment {
    field: SigningField,
    expected_len: Option<usize>,
    bytes_seen: usize,
}

/// Validates the field order and prefix byte bounds of a streamed `SignTransaction` preimage and
/// extracts its [`TxDisplay`] summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignStreamState {
    mode: SignMode,
    field_cursor: usize,
    in_field: Option<ActiveSegment>,
    display: TxDisplay,
}

const EXPECTED_SCHEMA_VERSION: u16 = 1;
const PUBLIC_KEY_BYTES_LEN: usize = 32;

impl SignStreamState {
    pub const fn new(mode: SignMode) -> Self {
        Self {
            mode,
            field_cursor: 0,
            in_field: None,
            display: TxDisplay {
                network: 0,
                fee_instruction_count: 0,
            },
        }
    }

    pub fn accept_segment(&mut self, p1: u8, data: &[u8]) -> Result<(), OotleStatusWord> {
        let is_last = (p1 & SEGMENT_LAST_CHUNK) != 0;
        let field = SigningField::try_from(p1 & !SEGMENT_LAST_CHUNK).map_err(|_| OotleStatusWord::BadRequest)?;
        let seq = self.mode.expected_fields();

        let mut segment = match self.in_field {
            None => {
                let expected = seq
                    .get(self.field_cursor)
                    .copied()
                    .ok_or(OotleStatusWord::SignStreamError)?;
                if field != expected {
                    return Err(OotleStatusWord::SignStreamError);
                }
                let expected_len = begin_segment(&mut self.display, field, data)?;
                ActiveSegment {
                    field,
                    expected_len,
                    bytes_seen: 0,
                }
            },
            Some(current) => {
                if field != current.field {
                    return Err(OotleStatusWord::SignStreamError);
                }
                current
            },
        };

        segment.bytes_seen = segment
            .bytes_seen
            .checked_add(data.len())
            .ok_or(OotleStatusWord::BadRequest)?;
        if let Some(expected) = segment.expected_len {
            if is_last && segment.bytes_seen != expected {
                return Err(OotleStatusWord::BadRequest);
            }
            if !is_last && segment.bytes_seen >= expected {
                return Err(OotleStatusWord::BadRequest);
            }
        }

        if is_last {
            self.field_cursor += 1;
            self.in_field = None;
        } else {
            self.in_field = Some(segment);
        }
        Ok(())
    }

    pub fn finish(self) -> Result<TxDisplay, OotleStatusWord> {
        if self.in_field.is_some() || self.field_cursor != self.mode.expected_fields().len() {
            return Err(OotleStatusWord::SignStreamError);
        }
        Ok(self.display)
    }
}

fn begin_segment(display: &mut TxDisplay, field: SigningField, data: &[u8]) -> Result<Option<usize>, OotleStatusWord> {
    match field {
        SigningField::SchemaVersion => {
            if read_u16(data)? != EXPECTED_SCHEMA_VERSION {
                return Err(OotleStatusWord::BadRequest);
            }
            Ok(Some(2))
        },
        SigningField::SealSigner => Ok(Some(PUBLIC_KEY_BYTES_LEN)),
        SigningField::Network => {
            display.network = *data.first().ok_or(OotleStatusWord::BadRequest)?;
            Ok(Some(1))
        },
        SigningField::FeeInstructions => {
            display.fee_instruction_count = read_u32(data)?;
            Ok(None)
        },
        _ => Ok(None),
    }
}

fn read_u16(data: &[u8]) -> Result<u16, OotleStatusWord> {
    let bytes: [u8; 2] = data
        .get(..2)
        .ok_or(OotleStatusWord::BadRequest)?
        .try_into()
        .map_err(|_| OotleStatusWord::BadRequest)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(data: &[u8]) -> Result<u32, OotleStatusWord> {
    let bytes: [u8; 4] = data
        .get(..4)
        .ok_or(OotleStatusWord::BadRequest)?
        .try_into()
        .map_err(|_| OotleStatusWord::BadRequest)?;
    Ok(u32::from_le_bytes(bytes))
}
