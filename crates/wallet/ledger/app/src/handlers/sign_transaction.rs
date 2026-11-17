//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Device-agnostic `SignTransaction` streaming state machine.
//!
//! A signing exchange is a sequence of APDUs:
//!   1. `Header` (P2 = [`FrameKind::Header`]) — derivation params + signing [`SignMode`].
//!   2. one `Segment` per canonical preimage field, in order (P2 = [`FrameKind::Segment`], P1 = field tag, high bit set
//!      on the field's last chunk). Bytes are fed verbatim into the message digest and the display summary is parsed
//!      from the same bytes.
//!   3. `Finalize` (P2 = [`FrameKind::Finalize`]) — yields the message + summary for user review.
//!
//! After the user approves, [`sign_approved`] derives the key and produces the signature. The
//! device-specific dispatch (bagl/nbgl) owns the transport and the review UI.

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use borsh::BorshDeserialize;
use ootle_ledger_common::{
    OotleStatusWord,
    arg_types::{
        FrameKind,
        KeyType,
        SignMode,
        SignStreamState,
        SignTransactionHeader,
        SignTransactionResponse,
        TxDisplay,
    },
};

use crate::{
    hashing::{MessageHasher, derive_stealth_secret, sign_message},
    key_derive::derive_from_bip32_key,
    state::{SigningState, State},
    status::AppStatus,
};

/// Everything needed to render the review screen and sign once approved.
pub struct SignReview {
    pub message: [u8; 64],
    pub account: u64,
    pub index: u64,
    pub key_type: KeyType,
    pub mode: SignMode,
    pub stealth_public_nonce: Option<[u8; 32]>,
    pub display: TxDisplay,
}

pub enum ChunkResult {
    /// Intermediate chunk consumed; reply with an empty OK.
    Ack,
    /// Stream complete; show the review UI, then call [`sign_approved`] if accepted.
    ReadyToSign(SignReview),
}

const fn stream_err() -> AppStatus {
    AppStatus::OotleStatusWord(OotleStatusWord::SignStreamError)
}

const fn bad_request() -> AppStatus {
    AppStatus::OotleStatusWord(OotleStatusWord::BadRequest)
}

const fn hash_fail() -> AppStatus {
    AppStatus::OotleStatusWord(OotleStatusWord::HashFail)
}

/// Process one `SignTransaction` APDU, identified by its `P1`/`P2` and data payload.
pub fn process_chunk(state: &mut State, p1: u8, p2: u8, data: &[u8]) -> Result<ChunkResult, AppStatus> {
    let result = FrameKind::try_from(p2)
        .map_err(|_| bad_request())
        .and_then(|frame_kind| match frame_kind {
            FrameKind::Header => {
                start_stream(state, data)?;
                Ok(ChunkResult::Ack)
            },
            FrameKind::Segment => {
                process_segment(state, p1, data)?;
                Ok(ChunkResult::Ack)
            },
            FrameKind::Finalize => finalize(state),
        });
    if result.is_err() {
        state.reset();
    }
    result
}

fn start_stream(state: &mut State, data: &[u8]) -> Result<(), AppStatus> {
    let header = SignTransactionHeader::try_from_slice(data).map_err(|_| bad_request())?;
    if header.stealth_public_nonce.is_some() && !matches!(header.key_type, KeyType::Account) {
        return Err(bad_request());
    }
    let hasher = MessageHasher::new(header.mode).map_err(|_| hash_fail())?;
    *state = State::SigningTransaction(SigningState {
        hasher,
        account: header.account,
        index: header.index,
        key_type: header.key_type,
        mode: header.mode,
        stealth_public_nonce: header.stealth_public_nonce,
        stream: SignStreamState::new(header.mode),
    });
    Ok(())
}

fn process_segment(state: &mut State, p1: u8, data: &[u8]) -> Result<(), AppStatus> {
    let signing = match state {
        State::SigningTransaction(s) => s,
        _ => return Err(stream_err()),
    };

    signing.stream.accept_segment(p1, data)?;
    signing.hasher.update(data).map_err(|_| hash_fail())?;
    Ok(())
}

fn finalize(state: &mut State) -> Result<ChunkResult, AppStatus> {
    // Take ownership and reset state up-front: a rejection or error must leave a clean slate.
    let signing = match core::mem::take(state) {
        State::SigningTransaction(s) => s,
        State::None => return Err(stream_err()),
    };

    let display = signing.stream.finish()?;
    let message = signing.hasher.finalize().map_err(|_| hash_fail())?;
    Ok(ChunkResult::ReadyToSign(SignReview {
        message,
        account: signing.account,
        index: signing.index,
        key_type: signing.key_type,
        mode: signing.mode,
        stealth_public_nonce: signing.stealth_public_nonce,
        display,
    }))
}

/// Build the `(label, value)` rows shown on the review screen. Device-agnostic; the bagl/nbgl
/// layers render these into their respective field widgets.
pub fn review_fields(review: &SignReview) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let operation = match (review.mode, review.stealth_public_nonce.is_some()) {
        (SignMode::AddSigner, false) => "Authorize",
        (SignMode::AddSigner, true) => "Authorize (confidential)",
        (SignMode::Seal, false) => "Seal",
        (SignMode::Seal, true) => "Seal (confidential)",
    };
    fields.push(("Operation".to_string(), operation.to_string()));
    fields.push(("Network".to_string(), review.display.network.to_string()));
    fields.push((
        "Fee instructions".to_string(),
        review.display.fee_instruction_count.to_string(),
    ));
    fields.push(("Tx digest".to_string(), to_hex(&review.message)));
    fields
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

/// Derive the signing key and sign the reviewed message. Called only after the user approves. For a
/// confidential transfer (`stealth_public_nonce` set) the account key is tweaked to the stealth key
/// `c + k` before signing, so the returned public key is the stealth address.
pub fn sign_approved(review: &SignReview) -> Result<SignTransactionResponse, AppStatus> {
    let account_secret = zeroize::Zeroizing::new(derive_from_bip32_key(review.account, review.index, review.key_type)?);
    let secret = match review.stealth_public_nonce {
        Some(nonce) => {
            let view_secret = zeroize::Zeroizing::new(derive_from_bip32_key(
                review.account,
                review.index,
                KeyType::ViewOnlyKey,
            )?);
            zeroize::Zeroizing::new(derive_stealth_secret(
                review.display.network,
                &account_secret,
                &view_secret,
                &nonce,
            )?)
        },
        None => account_secret,
    };
    let (public_key, signature) = sign_message(&secret, &review.message)?;
    Ok(SignTransactionResponse { public_key, signature })
}
