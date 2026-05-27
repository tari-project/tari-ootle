//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use borsh::{BorshDeserialize, BorshSerialize};

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
    pub fn as_u64(&self) -> u64 {
        *self as u64
    }
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct GetPublicKeyRequest {
    pub account: u64,
    pub index: u64,
    pub key_type: KeyType,
}

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
    IsSealSignerAuthorized = 8,
    DryRun = 9,
    BlobHashes = 10,
    Signatures = 11,
}

impl SigningField {
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
            8 => Ok(IsSealSignerAuthorized),
            9 => Ok(DryRun),
            10 => Ok(BlobHashes),
            11 => Ok(Signatures),
            _ => Err(()),
        }
    }
}

/// Body of the `Header` frame that opens a `SignTransaction` stream.
///
/// Identifies the signing key (same derivation params as [`GetPublicKeyRequest`]) and the
/// procedure. The transaction fields themselves follow as `Segment` frames.
#[derive(BorshSerialize, BorshDeserialize)]
pub struct SignTransactionHeader {
    pub account: u64,
    pub index: u64,
    pub key_type: KeyType,
    pub mode: SignMode,
}

/// Response returned on the `Finalize` frame once the user approves.
///
/// `signature` is the tari `SchnorrSignatureBytes` layout: `R.compress()(32) || s(32)`.
#[derive(BorshSerialize, BorshDeserialize)]
pub struct SignTransactionResponse {
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
}
