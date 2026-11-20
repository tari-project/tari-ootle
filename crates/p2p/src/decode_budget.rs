//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A type-aware pre-decode check that bounds how much heap memory decoding a protobuf payload can allocate.
//!
//! `prost` allocates a Rust struct or `Vec` element for every embedded message, `repeated bytes`/`string` entry, and
//! repeated scalar element it decodes. A `repeated` message field costs two bytes per empty element on the wire (tag
//! and zero length) yet tens or hundreds of bytes once decoded, so a wire-frame byte limit alone does not bound heap
//! growth: one 4 MiB frame of empty `Command` entries decodes into more than two million structs (hundreds of MiB).
//!
//! [`DecodeBudget`] counts the allocations a payload would make `prost` perform without allocating anything itself. It
//! follows the message schema: every length-delimited field under a message-typed tag charges one item and recurses
//! with that field's walker; each `repeated bytes` or `repeated string` element charges one item without entering its
//! bytes; a packed repeated scalar field charges its byte length (an upper bound on its element count); and each
//! unpacked occurrence of a repeated scalar field charges one item. Singular `bytes`, `string`, packed, and unknown
//! fields are never entered as messages, so arbitrary CBOR/BOR or cryptographic bytes cannot be mistaken for nested
//! messages.
//!
//! Unknown groups are skipped the way `prost` skips them so they cannot hide a flood that follows. Malformed wire data
//! (tag 0, invalid wire type, truncated varint, length running past the buffer, or malformed group) stops the walk
//! with `Ok(())` because `prost::Message::decode` rejects the payload at the same point before decoding anything past
//! it.

use std::fmt;

use crate::proto;

/// Maximum embedded items permitted in a single direct consensus `HotStuffMessage`.
///
/// A maximally populated valid `ProposalMessage` or `CatchUpSyncResponse` (`1,000` multi-shard commands with four
/// nested submessages each = `6,000` items, `BlockHeader` = `4`, `QuorumCertificate` with `1,000` signatures = `1,003`,
/// `TimeoutCertificate` with `1,000` signed timeouts = `2,003`, plus outer wrappers) uses under `10,020` items;
/// `MissingTransactionsRequest` and `MissingTransactionsResponse` at `1,000` entries use `1,001` items. `32,768`
/// provides more than 3x headroom while bounding worst-case empty-submessage amplification on a 4 MiB frame.
pub const HOTSTUFF_MAX_DECODE_ITEMS: usize = 32_768;

/// Maximum embedded items permitted on the network-wide `"consensus"` gossip topic.
///
/// Only `ForeignProposalNotification` (`1` outer message plus at most `256` `u32` `shard_groups`, costing at most
/// `1 + 256 * 5 = 1,281` items when packed as 5-byte varints or `257` items when unpacked) is broadcast on this topic.
pub const CONSENSUS_GOSSIP_MAX_DECODE_ITEMS: usize = 1_536;

/// Deepest message nesting the walk follows. Schema walks only reach this on recursive types; exceeding it is treated
/// as over budget.
const MAX_DEPTH: usize = 128;

/// The payload would make `prost` allocate more embedded items than allowed, or nests deeper than [`MAX_DEPTH`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeBudgetExceeded {
    /// Items counted when the walk stopped.
    pub items: usize,
    /// Configured budget.
    pub max: usize,
}

impl fmt::Display for DecodeBudgetExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "message exceeds the decode budget ({} embedded items, at most {} allowed)",
            self.items, self.max
        )
    }
}

impl std::error::Error for DecodeBudgetExceeded {}

/// Error returned when checking a payload's [`DecodeBudget`] and decoding it with `prost`.
#[derive(Debug)]
pub enum DecodeWithBudgetError {
    /// The payload exceeded the configured item or nesting budget before `prost` ran.
    BudgetExceeded(DecodeBudgetExceeded),
    /// The payload stayed within budget but failed `prost` wire decoding.
    Decode(prost::DecodeError),
}

impl fmt::Display for DecodeWithBudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BudgetExceeded(err) => err.fmt(f),
            Self::Decode(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for DecodeWithBudgetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BudgetExceeded(err) => Some(err),
            Self::Decode(err) => Some(err),
        }
    }
}

/// Running count of decoded allocations for one protobuf payload.
#[derive(Debug)]
pub struct Budget {
    items: usize,
    max: usize,
    depth: usize,
}

impl Budget {
    /// Creates a budget that allows at most `max` embedded items.
    pub fn new(max: usize) -> Self {
        Self {
            items: 0,
            max,
            depth: 0,
        }
    }

    /// Items counted so far.
    pub fn items(&self) -> usize {
        self.items
    }

    fn exceeded(&self) -> DecodeBudgetExceeded {
        DecodeBudgetExceeded {
            items: self.items,
            max: self.max,
        }
    }

    /// Charges `count` leaf items that are not walked (e.g. elements of `repeated bytes` or the byte length of a
    /// packed repeated scalar field).
    pub fn charge_items(&mut self, count: usize) -> Result<(), DecodeBudgetExceeded> {
        self.items = self.items.saturating_add(count);
        if self.items > self.max {
            return Err(self.exceeded());
        }
        Ok(())
    }

    /// Charges one instance of message `T`, encoded in `contents`, and walks its fields.
    pub fn charge_message<T: DecodeBudget + ?Sized>(&mut self, contents: &[u8]) -> Result<(), DecodeBudgetExceeded> {
        self.items = self.items.saturating_add(1);
        if self.items > self.max || self.depth >= MAX_DEPTH {
            return Err(self.exceeded());
        }
        self.depth = self.depth.saturating_add(1);
        let result = T::count_messages(contents, self);
        self.depth = self.depth.saturating_sub(1);
        result
    }

    /// Charges one map entry encoded in `contents` whose value (tag 2) is a message `V`, and walks that value.
    pub fn charge_map_entry<V: DecodeBudget + ?Sized>(&mut self, contents: &[u8]) -> Result<(), DecodeBudgetExceeded> {
        self.items = self.items.saturating_add(1);
        if self.items > self.max {
            return Err(self.exceeded());
        }
        walk_len_fields(contents, self, |tag, value, budget| {
            if tag == 2 {
                budget.charge_message::<V>(value)
            } else {
                Ok(())
            }
        })
    }
}

/// Counts the heap-allocated items decoding a protobuf message of type `Self` would make `prost` produce.
pub trait DecodeBudget {
    /// Walks one encoded instance of this message in `buf`, charging `budget` for every embedded allocation.
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded>;
}

macro_rules! impl_no_embedded_messages {
    ($($ty:ty),* $(,)?) => {
        $(
            impl DecodeBudget for $ty {
                fn count_messages(_buf: &[u8], _budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
                    Ok(())
                }
            }
        )*
    };
}

impl_no_embedded_messages!(
    (),
    bool,
    u32,
    u64,
    i32,
    i64,
    f32,
    f64,
    String,
    Vec<u8>,
    prost::bytes::Bytes,
    // proto::common leaf messages
    proto::common::Signature,
    proto::common::SignatureAndPublicKey,
    proto::common::SubstateAddress,
    proto::common::Epoch,
    proto::common::Amount,
    proto::common::CommitmentSignature,
    proto::common::OptionalVersion,
    // proto::transaction leaf messages
    proto::transaction::Transaction,
    // proto::consensus leaf messages
    proto::consensus::ForeignProposalRequestByBlockId,
    proto::consensus::ForeignProposal,
    proto::consensus::ExtraData,
    proto::consensus::LeaderFee,
    proto::consensus::EndEpochAtom,
    proto::consensus::ForeignProposalAtom,
    proto::consensus::LockedEpoch,
    proto::consensus::Decision,
    proto::consensus::Evidence,
    proto::consensus::UpState,
    proto::consensus::DownState,
    proto::consensus::SyncRequest,
    proto::consensus::ShardGroupAccumulatedData,
);

/// Checks that decoding `buf` as `T` allocates at most `max_items` embedded items, returning the counted total.
pub fn check_decode_budget<T: DecodeBudget + ?Sized>(
    buf: &[u8],
    max_items: usize,
) -> Result<usize, DecodeBudgetExceeded> {
    let mut budget = Budget::new(max_items);
    T::count_messages(buf, &mut budget)?;
    Ok(budget.items())
}

/// Decodes `buf` as `T` after checking with [`check_decode_budget`] that it carries at most `max_items` embedded items.
pub fn decode_with_max_items<T>(buf: &[u8], max_items: usize) -> Result<T, DecodeWithBudgetError>
where T: prost::Message + Default + DecodeBudget {
    check_decode_budget::<T>(buf, max_items).map_err(DecodeWithBudgetError::BudgetExceeded)?;
    T::decode(buf).map_err(DecodeWithBudgetError::Decode)
}

/// Calls `f(tag, contents, budget)` for every length-delimited field in `buf`, skipping all other well-formed fields
/// (including groups, matching `prost`). Stops with `Ok(())` at the first malformed field.
pub fn walk_len_fields<F>(buf: &[u8], budget: &mut Budget, f: F) -> Result<(), DecodeBudgetExceeded>
where F: FnMut(u32, &[u8], &mut Budget) -> Result<(), DecodeBudgetExceeded> {
    walk_fields(buf, budget, &[], &[], f)
}

/// Walks a message with map fields under `map_tags` and repeated scalar fields under `repeated_scalar_tags`.
pub fn walk_fields<F>(
    buf: &[u8],
    budget: &mut Budget,
    map_tags: &[u32],
    repeated_scalar_tags: &[u32],
    mut f: F,
) -> Result<(), DecodeBudgetExceeded>
where
    F: FnMut(u32, &[u8], &mut Budget) -> Result<(), DecodeBudgetExceeded>,
{
    let mut pos = 0usize;
    while pos < buf.len() {
        let Some(key) = read_varint(buf, &mut pos) else {
            return Ok(());
        };
        let Ok(tag) = u32::try_from(key >> 3) else {
            return Ok(());
        };
        if tag == 0 {
            return Ok(());
        }
        let wire_type = key & 0x7;
        if wire_type > 5 {
            return Ok(());
        }
        let wire_type = if map_tags.contains(&tag) { 2 } else { wire_type };
        if matches!(wire_type, 0 | 1 | 5) && repeated_scalar_tags.contains(&tag) {
            budget.charge_items(1)?;
        }
        let skip = match wire_type {
            0 => {
                if read_varint(buf, &mut pos).is_none() {
                    return Ok(());
                }
                0
            },
            1 => 8,
            2 => {
                let Some(len) = read_varint(buf, &mut pos).and_then(|len| usize::try_from(len).ok()) else {
                    return Ok(());
                };
                let Some(contents) = pos.checked_add(len).and_then(|end| buf.get(pos..end)) else {
                    return Ok(());
                };
                f(tag, contents, budget)?;
                len
            },
            3 => {
                if !skip_group(buf, &mut pos, tag, 0) {
                    return Ok(());
                }
                0
            },
            5 => 4,
            _ => return Ok(()),
        };
        match pos.checked_add(skip).filter(|end| *end <= buf.len()) {
            Some(end) => pos = end,
            None => return Ok(()),
        }
    }
    Ok(())
}

fn skip_group(buf: &[u8], pos: &mut usize, group_tag: u32, depth: usize) -> bool {
    if depth >= MAX_DEPTH {
        return false;
    }
    loop {
        let Some(key) = read_varint(buf, pos) else {
            return false;
        };
        let Ok(tag) = u32::try_from(key >> 3) else {
            return false;
        };
        if tag == 0 {
            return false;
        }
        let skip = match key & 0x7 {
            0 => {
                if read_varint(buf, pos).is_none() {
                    return false;
                }
                0
            },
            1 => 8,
            2 => match read_varint(buf, pos).and_then(|len| usize::try_from(len).ok()) {
                Some(len) => len,
                None => return false,
            },
            3 => {
                if !skip_group(buf, pos, tag, depth.saturating_add(1)) {
                    return false;
                }
                0
            },
            4 => return tag == group_tag,
            5 => 4,
            _ => return false,
        };
        match pos.checked_add(skip).filter(|end| *end <= buf.len()) {
            Some(end) => *pos = end,
            None => return false,
        }
    }
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for i in 0..10u32 {
        let byte = *buf.get(*pos)?;
        *pos = pos.checked_add(1)?;
        value |= u64::from(byte & 0x7f).checked_shl(i.checked_mul(7)?)?;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

// -------------------------------- Schema implementations -------------------------------- //

impl DecodeBudget for proto::common::SubstateRequirement {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            2 => budget.charge_message::<proto::common::OptionalVersion>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::transaction::NewTransactionMessage {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => budget.charge_message::<proto::transaction::Transaction>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::HotStuffMessage {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => budget.charge_message::<proto::consensus::NewViewMessage>(contents),
            2 | 10 => budget.charge_message::<proto::consensus::ProposalMessage>(contents),
            3 => budget.charge_message::<proto::consensus::ForeignProposalMessage>(contents),
            4 => budget.charge_message::<proto::consensus::ForeignProposalNotification>(contents),
            5 => budget.charge_message::<proto::consensus::ForeignProposalRequest>(contents),
            6 => budget.charge_message::<proto::consensus::VoteMessage>(contents),
            7 => budget.charge_message::<proto::consensus::MissingTransactionsRequest>(contents),
            8 => budget.charge_message::<proto::consensus::MissingTransactionsResponse>(contents),
            9 => budget.charge_message::<proto::consensus::SyncRequest>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::NewViewMessage {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => budget.charge_message::<proto::consensus::QuorumCertificate>(contents),
            2 => budget.charge_message::<proto::consensus::VoteMessage>(contents),
            3 => budget.charge_message::<proto::consensus::TimeoutVote>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::ProposalMessage {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => budget.charge_message::<proto::consensus::Block>(contents),
            2 => budget.charge_message::<proto::consensus::ForeignProposal>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::TimeoutVote {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            3 => budget.charge_message::<proto::common::SignatureAndPublicKey>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::ForeignProposalMessage {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => budget.charge_message::<proto::consensus::ForeignProposal>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::ForeignProposalNotification {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_fields(buf, budget, &[], &[3], |tag, contents, budget| match tag {
            3 => budget.charge_items(contents.len()),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::ForeignProposalRequest {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => budget.charge_message::<proto::consensus::ForeignProposalRequestByBlockId>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::VoteMessage {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            5 => budget.charge_message::<proto::common::SignatureAndPublicKey>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::BlockHeader {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            10 => budget.charge_message::<proto::common::Signature>(contents),
            13 => budget.charge_message::<proto::consensus::ExtraData>(contents),
            14 => budget.charge_message::<proto::consensus::ShardGroupAccumulatedData>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::Block {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => budget.charge_message::<proto::consensus::BlockHeader>(contents),
            2 => budget.charge_message::<proto::consensus::QuorumCertificate>(contents),
            3 => budget.charge_message::<proto::consensus::Command>(contents),
            4 => budget.charge_message::<proto::consensus::TimeoutCertificate>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::ShardStateVersions {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_fields(buf, budget, &[], &[1], |tag, contents, budget| match tag {
            1 => budget.charge_items(contents.len()),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::Command {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => budget.charge_message::<proto::consensus::LocalOnlyAtom>(contents),
            2..=5 => budget.charge_message::<proto::consensus::MultiShardAtom>(contents),
            6 => budget.charge_message::<proto::consensus::ForeignProposalAtom>(contents),
            7 => budget.charge_message::<proto::consensus::EndEpochAtom>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::MultiShardAtom {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            3 => budget.charge_message::<proto::consensus::Decision>(contents),
            4 => budget.charge_message::<proto::consensus::Evidence>(contents),
            6 => budget.charge_message::<proto::consensus::LeaderFee>(contents),
            8 => budget.charge_message::<proto::consensus::LockedEpoch>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::LocalOnlyAtom {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            2 => budget.charge_message::<proto::consensus::Decision>(contents),
            4 => budget.charge_message::<proto::consensus::LeaderFee>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::QuorumCertificate {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            9 => budget.charge_message::<proto::consensus::QcSignatures>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::QcSignatures {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => budget.charge_message::<proto::consensus::SchnorrQcSignatures>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::SchnorrQcSignatures {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            2 => budget.charge_message::<proto::common::Signature>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::TimeoutCertificate {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            3 => budget.charge_message::<proto::consensus::TcSignatures>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::TcSignatures {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => budget.charge_message::<proto::consensus::SchnorrTcSignatures>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::SchnorrTcSignatures {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            2 => budget.charge_message::<proto::consensus::SignedTimeout>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::SignedTimeout {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            2 => budget.charge_message::<proto::common::Signature>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::ValidatorMetadata {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            3 => budget.charge_message::<proto::common::Signature>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::TariDanPayload {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => budget.charge_message::<proto::transaction::Transaction>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::SubstateState {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            2 => budget.charge_message::<proto::consensus::UpState>(contents),
            3 => budget.charge_message::<proto::consensus::DownState>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::MissingTransactionsRequest {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, _contents, budget| match tag {
            4 => budget.charge_items(1),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::MissingTransactionsResponse {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            4 => budget.charge_message::<proto::transaction::Transaction>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::Substate {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            4 => budget.charge_message::<proto::consensus::SubstateCreatedMetadata>(contents),
            5 => budget.charge_message::<proto::consensus::SubstateDestroyedMetadata>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::SubstateCreatedMetadata {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => budget.charge_message::<proto::common::Epoch>(contents),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::SubstateDestroyedMetadata {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => budget.charge_message::<proto::common::Epoch>(contents),
            _ => Ok(()),
        })
    }
}

const _: fn(proto::consensus::HotStuffMessage) = assert_hotstuff_schema_exhaustive;

/// Compile-time witness ensuring every field and `oneof` variant reachable from `HotStuffMessage` is accounted for in
/// the walkers above: adding a field or variant in `consensus.proto`, `common.proto`, or `transaction.proto` fails to
/// compile here until its [`DecodeBudget`] implementation is updated.
fn assert_hotstuff_schema_exhaustive(msg: proto::consensus::HotStuffMessage) {
    use proto::consensus::{
        ForeignProposal,
        ForeignProposalMessage,
        ForeignProposalNotification,
        ForeignProposalRequest,
        ForeignProposalRequestByBlockId,
        HotStuffMessage,
        MissingTransactionsRequest,
        MissingTransactionsResponse,
        NewViewMessage,
        SyncRequest,
        TimeoutVote,
        foreign_proposal_request::Request as ForeignRequestKind,
        hot_stuff_message::Message as HotStuffKind,
    };

    let HotStuffMessage { message } = msg;
    let Some(kind) = message else {
        return;
    };
    match kind {
        HotStuffKind::NewView(NewViewMessage {
            high_qc,
            last_vote,
            timeout,
        }) => {
            if let Some(qc) = high_qc {
                witness_qc(qc);
            }
            if let Some(vote) = last_vote {
                witness_vote(vote);
            }
            if let Some(TimeoutVote {
                epoch: _,
                height: _,
                signature: Some(sig),
                high_pc_height: _,
            }) = timeout
            {
                witness_sig_pk(sig);
            }
        },
        HotStuffKind::Proposal(p) | HotStuffKind::CatchUpSyncResponse(p) => witness_proposal(p),
        HotStuffKind::ForeignProposal(ForeignProposalMessage { proposal }) => {
            if let Some(ForeignProposal {
                encoded_commit_proof: _,
                encoded_block_pledge: _,
                encoded_block_metadata: _,
            }) = proposal
            {}
        },
        HotStuffKind::ForeignProposalNotification(ForeignProposalNotification {
            block_id: _,
            epoch: _,
            shard_groups: _,
            shard_group: _,
        }) => {},
        HotStuffKind::ForeignProposalRequest(ForeignProposalRequest {
            request:
                Some(ForeignRequestKind::ByBlockId(ForeignProposalRequestByBlockId {
                    block_id: _,
                    for_shard_group: _,
                    epoch: _,
                })),
        }) |
        HotStuffKind::ForeignProposalRequest(ForeignProposalRequest { request: None }) => {},
        HotStuffKind::Vote(vote) => witness_vote(vote),
        HotStuffKind::RequestMissingTransactions(MissingTransactionsRequest {
            request_id: _,
            epoch: _,
            block_id: _,
            transaction_ids: _,
        }) => {},
        HotStuffKind::RequestedTransaction(MissingTransactionsResponse {
            request_id: _,
            epoch: _,
            block_id: _,
            transactions,
        }) => for proto::transaction::Transaction { bor_encoded: _ } in transactions {},
        HotStuffKind::SyncRequest(SyncRequest {
            epoch: _,
            block_height: _,
        }) => {},
    }
}

fn witness_sig(sig: proto::common::Signature) {
    let proto::common::Signature {
        public_nonce: _,
        signature: _,
    } = sig;
}

fn witness_sig_pk(sig: proto::common::SignatureAndPublicKey) {
    let proto::common::SignatureAndPublicKey {
        public_nonce: _,
        signature: _,
        public_key: _,
    } = sig;
}

fn witness_qc(qc: proto::consensus::QuorumCertificate) {
    use proto::consensus::{QcSignatures, QuorumCertificate, SchnorrQcSignatures, qc_signatures::Scheme};
    let QuorumCertificate {
        header_hash: _,
        parent_id: _,
        block_height: _,
        epoch: _,
        decision: _,
        shard_group: _,
        signatures,
    } = qc;
    if let Some(QcSignatures {
        scheme: Some(Scheme::Schnorr(SchnorrQcSignatures { signers: _, signatures })),
    }) = signatures
    {
        for sig in signatures {
            witness_sig(sig);
        }
    }
}

fn witness_tc(tc: proto::consensus::TimeoutCertificate) {
    use proto::consensus::{
        SchnorrTcSignatures,
        SignedTimeout,
        TcSignatures,
        TimeoutCertificate,
        tc_signatures::Scheme,
    };
    let TimeoutCertificate {
        epoch: _,
        block_height: _,
        signatures,
    } = tc;
    if let Some(TcSignatures {
        scheme: Some(Scheme::Schnorr(SchnorrTcSignatures { signers: _, timeouts })),
    }) = signatures
    {
        for SignedTimeout {
            high_pc_height: _,
            signature,
        } in timeouts
        {
            if let Some(sig) = signature {
                witness_sig(sig);
            }
        }
    }
}

fn witness_vote(vote: proto::consensus::VoteMessage) {
    let proto::consensus::VoteMessage {
        epoch: _,
        block_id: _,
        block_height: _,
        decision: _,
        signature,
    } = vote;
    if let Some(sig) = signature {
        witness_sig_pk(sig);
    }
}

fn witness_decision(decision: proto::consensus::Decision) {
    use proto::consensus::decision::Decision as Kind;
    let proto::consensus::Decision { decision } = decision;
    if let Some(kind) = decision {
        match kind {
            Kind::Commit(_) | Kind::Abort(_) => {},
        }
    }
}

fn witness_header(header: proto::consensus::BlockHeader) {
    use proto::consensus::{BlockHeader, ExtraData, ShardGroupAccumulatedData};
    let BlockHeader {
        parent_id: _,
        network: _,
        height: _,
        epoch: _,
        shard_group: _,
        proposed_by: _,
        state_merkle_root: _,
        total_leader_fee: _,
        signature,
        timestamp: _,
        epoch_hash: _,
        extra_data,
        accumulated_data,
        protocol_version: _,
    } = header;
    if let Some(sig) = signature {
        witness_sig(sig);
    }
    if let Some(ExtraData { encoded_extra_data: _ }) = extra_data {}
    if let Some(ShardGroupAccumulatedData {
        total_exhaust_burn_msb: _,
        total_exhaust_burn_lsb: _,
    }) = accumulated_data
    {}
}

fn witness_command(cmd: proto::consensus::Command) {
    use proto::consensus::{
        Command,
        EndEpochAtom,
        Evidence,
        ForeignProposalAtom,
        LeaderFee,
        LocalOnlyAtom,
        LockedEpoch,
        MultiShardAtom,
        command::Command as Kind,
    };
    let Command { command } = cmd;
    let Some(kind) = command else {
        return;
    };
    match kind {
        Kind::LocalOnly(LocalOnlyAtom {
            id: _,
            decision,
            fee: _,
            leader_fee,
        }) => {
            if let Some(d) = decision {
                witness_decision(d);
            }
            if let Some(LeaderFee {
                leader_fee: _,
                exhaust_burn: _,
            }) = leader_fee
            {}
        },
        Kind::LocalPrepare(atom) | Kind::LocalAccept(atom) | Kind::AllAccept(atom) | Kind::SomeAccept(atom) => {
            let MultiShardAtom {
                id: _,
                decision,
                evidence,
                fee: _,
                leader_fee,
                fee_intent_only: _,
                locked_epoch,
            } = atom;
            if let Some(d) = decision {
                witness_decision(d);
            }
            if let Some(Evidence { encoded_evidence: _ }) = evidence {}
            if let Some(LeaderFee {
                leader_fee: _,
                exhaust_burn: _,
            }) = leader_fee
            {}
            if let Some(LockedEpoch {
                epoch: _,
                hash: _,
                exhaust_burn_rate_bps: _,
            }) = locked_epoch
            {}
        },
        Kind::ForeignProposal(ForeignProposalAtom {
            block_id: _,
            shard_group: _,
        }) => {},
        Kind::EndEpoch(EndEpochAtom { next_epoch_hash: _ }) => {},
    }
}

fn witness_proposal(proposal: proto::consensus::ProposalMessage) {
    use proto::consensus::{Block, ForeignProposal, ProposalMessage};
    let ProposalMessage {
        block,
        foreign_proposals,
    } = proposal;
    if let Some(Block {
        header,
        justify,
        commands,
        timeout_certificate,
    }) = block
    {
        if let Some(h) = header {
            witness_header(h);
        }
        if let Some(qc) = justify {
            witness_qc(qc);
        }
        for cmd in commands {
            witness_command(cmd);
        }
        if let Some(tc) = timeout_certificate {
            witness_tc(tc);
        }
    }
    for ForeignProposal {
        encoded_commit_proof: _,
        encoded_block_pledge: _,
        encoded_block_metadata: _,
    } in foreign_proposals
    {}
}

#[cfg(test)]
mod tests {
    use prost::Message;
    use tari_consensus::messages::MAX_REQUESTED_TRANSACTIONS;
    use tari_ootle_common_types::{ShardGroup, shard::Shard};
    use tari_swarm::messaging::MAX_MESSAGE_SIZE;

    use super::*;

    fn count<T: DecodeBudget>(msg: &impl Message, max: usize) -> Result<usize, DecodeBudgetExceeded> {
        check_decode_budget::<T>(&msg.encode_to_vec(), max)
    }

    fn fake_nested_messages(len: usize) -> Vec<u8> {
        [0x0a, 0x00].repeat(len / 2)
    }

    fn len_field(tag: u32, contents: &[u8]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(contents.len().saturating_add(16));
        prost::encoding::encode_key(tag, prost::encoding::WireType::LengthDelimited, &mut buf);
        prost::encoding::encode_varint(contents.len() as u64, &mut buf);
        buf.extend_from_slice(contents);
        buf
    }

    fn empty_elements(tag: u32, count: usize) -> Vec<u8> {
        len_field(tag, &[]).repeat(count)
    }

    #[derive(Clone, PartialEq, Message)]
    struct RecursiveNode {
        #[prost(message, optional, boxed, tag = "1")]
        child: Option<Box<RecursiveNode>>,
    }

    impl DecodeBudget for RecursiveNode {
        fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
            walk_len_fields(buf, budget, |tag, contents, budget| match tag {
                1 => budget.charge_message::<RecursiveNode>(contents),
                _ => Ok(()),
            })
        }
    }

    fn populated_multi_shard_command() -> proto::consensus::Command {
        proto::consensus::Command {
            command: Some(proto::consensus::command::Command::LocalPrepare(
                proto::consensus::MultiShardAtom {
                    id: vec![0x11; 32],
                    decision: Some(proto::consensus::Decision {
                        decision: Some(proto::consensus::decision::Decision::Commit(true)),
                    }),
                    evidence: Some(proto::consensus::Evidence {
                        encoded_evidence: vec![0x22; 16],
                    }),
                    fee: 100,
                    leader_fee: Some(proto::consensus::LeaderFee {
                        leader_fee: 90,
                        exhaust_burn: 10,
                    }),
                    fee_intent_only: false,
                    locked_epoch: Some(proto::consensus::LockedEpoch {
                        epoch: 3,
                        hash: vec![0x33; 32],
                        exhaust_burn_rate_bps: 500,
                    }),
                },
            )),
        }
    }

    fn populated_qc(num_signatures: usize) -> proto::consensus::QuorumCertificate {
        proto::consensus::QuorumCertificate {
            header_hash: vec![1; 32],
            parent_id: vec![2; 32],
            block_height: 10,
            epoch: 1,
            decision: 1,
            shard_group: 1,
            signatures: Some(proto::consensus::QcSignatures {
                scheme: Some(proto::consensus::qc_signatures::Scheme::Schnorr(
                    proto::consensus::SchnorrQcSignatures {
                        signers: vec![0xff; 128],
                        signatures: vec![
                            proto::common::Signature {
                                public_nonce: vec![3; 32],
                                signature: vec![4; 32],
                            };
                            num_signatures
                        ],
                    },
                )),
            }),
        }
    }

    fn populated_tc(num_timeouts: usize) -> proto::consensus::TimeoutCertificate {
        proto::consensus::TimeoutCertificate {
            epoch: 1,
            block_height: 10,
            signatures: Some(proto::consensus::TcSignatures {
                scheme: Some(proto::consensus::tc_signatures::Scheme::Schnorr(
                    proto::consensus::SchnorrTcSignatures {
                        signers: vec![0xff; 128],
                        timeouts: vec![
                            proto::consensus::SignedTimeout {
                                high_pc_height: 9,
                                signature: Some(proto::common::Signature {
                                    public_nonce: vec![5; 32],
                                    signature: vec![6; 32],
                                }),
                            };
                            num_timeouts
                        ],
                    },
                )),
            }),
        }
    }

    #[test]
    fn counts_every_hotstuff_variant_accurately() {
        use proto::consensus::hot_stuff_message::Message as M;

        let new_view = proto::consensus::HotStuffMessage {
            message: Some(M::NewView(proto::consensus::NewViewMessage {
                high_qc: Some(populated_qc(2)),
                last_vote: Some(proto::consensus::VoteMessage {
                    epoch: 1,
                    block_id: vec![1; 32],
                    block_height: 2,
                    decision: 1,
                    signature: Some(proto::common::SignatureAndPublicKey::default()),
                }),
                timeout: Some(proto::consensus::TimeoutVote {
                    epoch: 1,
                    height: 2,
                    signature: Some(proto::common::SignatureAndPublicKey::default()),
                    high_pc_height: 1,
                }),
            })),
        };
        // 1 NewView + (1 QC + 1 QcSignatures + 1 Schnorr + 2 Signature) + (1 Vote + 1 SigPk) + (1 Timeout + 1 SigPk)
        assert_eq!(count::<proto::consensus::HotStuffMessage>(&new_view, 100).unwrap(), 10);

        let foreign_req = proto::consensus::HotStuffMessage {
            message: Some(M::ForeignProposalRequest(proto::consensus::ForeignProposalRequest {
                request: Some(proto::consensus::foreign_proposal_request::Request::ByBlockId(
                    proto::consensus::ForeignProposalRequestByBlockId {
                        block_id: vec![7; 32],
                        for_shard_group: 1,
                        epoch: 2,
                    },
                )),
            })),
        };
        assert_eq!(
            count::<proto::consensus::HotStuffMessage>(&foreign_req, 100).unwrap(),
            2
        );

        let missing_req = proto::consensus::HotStuffMessage {
            message: Some(M::RequestMissingTransactions(
                proto::consensus::MissingTransactionsRequest {
                    request_id: 1,
                    epoch: 1,
                    block_id: vec![1; 32],
                    transaction_ids: vec![vec![9; 32]; MAX_REQUESTED_TRANSACTIONS],
                },
            )),
        };
        assert_eq!(
            count::<proto::consensus::HotStuffMessage>(&missing_req, HOTSTUFF_MAX_DECODE_ITEMS).unwrap(),
            1 + MAX_REQUESTED_TRANSACTIONS
        );

        let missing_resp = proto::consensus::HotStuffMessage {
            message: Some(M::RequestedTransaction(proto::consensus::MissingTransactionsResponse {
                request_id: 1,
                epoch: 1,
                block_id: vec![1; 32],
                transactions: vec![
                    proto::transaction::Transaction {
                        bor_encoded: vec![0xaa; 64],
                    };
                    MAX_REQUESTED_TRANSACTIONS
                ],
            })),
        };
        assert_eq!(
            count::<proto::consensus::HotStuffMessage>(&missing_resp, HOTSTUFF_MAX_DECODE_ITEMS).unwrap(),
            1 + MAX_REQUESTED_TRANSACTIONS
        );

        let sync_req = proto::consensus::HotStuffMessage {
            message: Some(M::SyncRequest(proto::consensus::SyncRequest {
                epoch: 2,
                block_height: 42,
            })),
        };
        assert_eq!(count::<proto::consensus::HotStuffMessage>(&sync_req, 10).unwrap(), 1);
    }

    #[test]
    fn admits_maximally_populated_valid_proposal_with_headroom() {
        const MAX_BLOCK_COMMANDS: usize = 1_000;
        let max_sigs = tari_sidechain::MAX_QC_SIGNATURES;

        let proposal = proto::consensus::ProposalMessage {
            block: Some(proto::consensus::Block {
                header: Some(proto::consensus::BlockHeader {
                    parent_id: vec![1; 32],
                    network: 0,
                    height: 100,
                    epoch: 5,
                    shard_group: 1,
                    proposed_by: vec![2; 32],
                    state_merkle_root: vec![3; 32],
                    total_leader_fee: 500,
                    signature: Some(proto::common::Signature {
                        public_nonce: vec![4; 32],
                        signature: vec![5; 32],
                    }),
                    timestamp: 1_700_000_000,
                    epoch_hash: vec![6; 32],
                    extra_data: Some(proto::consensus::ExtraData {
                        encoded_extra_data: vec![7; 32],
                    }),
                    accumulated_data: Some(proto::consensus::ShardGroupAccumulatedData {
                        total_exhaust_burn_msb: 0,
                        total_exhaust_burn_lsb: 100,
                    }),
                    protocol_version: 1,
                }),
                justify: Some(populated_qc(max_sigs)),
                commands: vec![populated_multi_shard_command(); MAX_BLOCK_COMMANDS],
                timeout_certificate: Some(populated_tc(max_sigs)),
            }),
            foreign_proposals: vec![
                proto::consensus::ForeignProposal {
                    encoded_commit_proof: vec![8; 64],
                    encoded_block_pledge: vec![9; 64],
                    encoded_block_metadata: vec![10; 64],
                };
                Shard::max().as_u32() as usize
            ],
        };

        for msg in [
            proto::consensus::HotStuffMessage {
                message: Some(proto::consensus::hot_stuff_message::Message::Proposal(proposal.clone())),
            },
            proto::consensus::HotStuffMessage {
                message: Some(proto::consensus::hot_stuff_message::Message::CatchUpSyncResponse(
                    proposal,
                )),
            },
        ] {
            let items = count::<proto::consensus::HotStuffMessage>(&msg, HOTSTUFF_MAX_DECODE_ITEMS).unwrap();
            assert!(
                items * 2 < HOTSTUFF_MAX_DECODE_ITEMS,
                "maximally populated proposal used {items} items against cap {HOTSTUFF_MAX_DECODE_ITEMS}"
            );
        }
    }

    #[test]
    fn admits_max_shard_groups_foreign_proposal_notification_packed_and_unpacked() {
        let max_shards = Shard::max().as_u32();
        let shard_groups = (1..=max_shards)
            .map(|s| ShardGroup::new(s, s).encode_as_u32())
            .collect::<Vec<_>>();
        let msg = proto::consensus::HotStuffMessage {
            message: Some(
                proto::consensus::hot_stuff_message::Message::ForeignProposalNotification(
                    proto::consensus::ForeignProposalNotification {
                        block_id: vec![0xab; 32],
                        epoch: u64::MAX,
                        shard_groups: shard_groups.clone(),
                        shard_group: ShardGroup::new(1u32, max_shards).encode_as_u32(),
                    },
                ),
            ),
        };
        let packed_items = count::<proto::consensus::HotStuffMessage>(&msg, CONSENSUS_GOSSIP_MAX_DECODE_ITEMS).unwrap();
        assert!(packed_items <= CONSENSUS_GOSSIP_MAX_DECODE_ITEMS);

        // Even 256 worst-case 5-byte u32::MAX varints stay within the gossip decode budget.
        let worst_varint_msg = proto::consensus::HotStuffMessage {
            message: Some(
                proto::consensus::hot_stuff_message::Message::ForeignProposalNotification(
                    proto::consensus::ForeignProposalNotification {
                        block_id: vec![0xab; 32],
                        epoch: u64::MAX,
                        shard_groups: vec![u32::MAX; max_shards as usize],
                        shard_group: u32::MAX,
                    },
                ),
            ),
        };
        assert!(
            count::<proto::consensus::HotStuffMessage>(&worst_varint_msg, CONSENSUS_GOSSIP_MAX_DECODE_ITEMS).is_ok()
        );

        // Unpacked repeated uint32 elements are charged one item per element.
        let mut unpacked_notif = len_field(1, &[0xab; 32]);
        for sg in &shard_groups {
            prost::encoding::encode_key(3, prost::encoding::WireType::Varint, &mut unpacked_notif);
            prost::encoding::encode_varint(u64::from(*sg), &mut unpacked_notif);
        }
        let outer = len_field(4, &unpacked_notif);
        let unpacked_items =
            check_decode_budget::<proto::consensus::HotStuffMessage>(&outer, CONSENSUS_GOSSIP_MAX_DECODE_ITEMS)
                .unwrap();
        assert_eq!(unpacked_items, 1 + shard_groups.len());
    }

    #[test]
    fn never_walks_bytes_fields_containing_fake_message_tags() {
        let blob = fake_nested_messages(256 * 1024);
        let msg = proto::consensus::HotStuffMessage {
            message: Some(proto::consensus::hot_stuff_message::Message::ForeignProposal(
                proto::consensus::ForeignProposalMessage {
                    proposal: Some(proto::consensus::ForeignProposal {
                        encoded_commit_proof: blob.clone(),
                        encoded_block_pledge: blob.clone(),
                        encoded_block_metadata: blob,
                    }),
                },
            )),
        };
        assert_eq!(count::<proto::consensus::HotStuffMessage>(&msg, 2).unwrap(), 2);
    }

    #[test]
    fn rejects_wire_floods_across_every_repeated_consensus_vector() {
        let flood_count = MAX_MESSAGE_SIZE / 2 - 16;

        // ProposalMessage (tag 2) -> Block (tag 1) -> repeated Command (tag 3)
        let cmd_flood = len_field(2, &len_field(1, &empty_elements(3, flood_count)));
        assert!(cmd_flood.len() <= MAX_MESSAGE_SIZE);
        let err = decode_with_max_items::<proto::consensus::HotStuffMessage>(&cmd_flood, HOTSTUFF_MAX_DECODE_ITEMS)
            .unwrap_err();
        assert!(err.to_string().contains("exceeds the decode budget"), "{err}");

        // ProposalMessage (tag 2) -> repeated ForeignProposal (tag 2)
        let fp_flood = len_field(2, &empty_elements(2, flood_count));
        assert!(
            check_decode_budget::<proto::consensus::HotStuffMessage>(&fp_flood, HOTSTUFF_MAX_DECODE_ITEMS).is_err()
        );

        // MissingTransactionsRequest (tag 7) -> repeated bytes transaction_ids (tag 4)
        let tx_id_flood = len_field(7, &empty_elements(4, flood_count));
        let err = check_decode_budget::<proto::consensus::HotStuffMessage>(&tx_id_flood, HOTSTUFF_MAX_DECODE_ITEMS)
            .unwrap_err();
        assert_eq!(err, DecodeBudgetExceeded {
            items: HOTSTUFF_MAX_DECODE_ITEMS + 1,
            max: HOTSTUFF_MAX_DECODE_ITEMS,
        });

        // MissingTransactionsResponse (tag 8) -> repeated Transaction (tag 4)
        let tx_flood = len_field(8, &empty_elements(4, flood_count));
        assert!(
            check_decode_budget::<proto::consensus::HotStuffMessage>(&tx_flood, HOTSTUFF_MAX_DECODE_ITEMS).is_err()
        );

        // NewView (tag 1) -> QuorumCertificate (tag 1) -> QcSignatures (tag 9) -> Schnorr (tag 1) -> Signature (tag 2)
        let qc_sig_flood = len_field(
            1,
            &len_field(1, &len_field(9, &len_field(1, &empty_elements(2, flood_count)))),
        );
        assert!(
            check_decode_budget::<proto::consensus::HotStuffMessage>(&qc_sig_flood, HOTSTUFF_MAX_DECODE_ITEMS).is_err()
        );

        // ForeignProposalNotification (tag 4) -> packed & unpacked shard_groups (tag 3)
        let packed_sg_flood = len_field(4, &len_field(3, &vec![1u8; CONSENSUS_GOSSIP_MAX_DECODE_ITEMS + 1]));
        assert!(
            check_decode_budget::<proto::consensus::HotStuffMessage>(
                &packed_sg_flood,
                CONSENSUS_GOSSIP_MAX_DECODE_ITEMS
            )
            .is_err()
        );
        let mut unpacked_sg_elem = Vec::new();
        prost::encoding::encode_key(3, prost::encoding::WireType::Varint, &mut unpacked_sg_elem);
        prost::encoding::encode_varint(1, &mut unpacked_sg_elem);
        let unpacked_sg_flood = len_field(4, &unpacked_sg_elem.repeat(CONSENSUS_GOSSIP_MAX_DECODE_ITEMS + 1));
        assert!(
            check_decode_budget::<proto::consensus::HotStuffMessage>(
                &unpacked_sg_flood,
                CONSENSUS_GOSSIP_MAX_DECODE_ITEMS
            )
            .is_err()
        );
    }

    #[test]
    fn unknown_groups_do_not_hide_a_following_flood_and_malformed_wire_stops_cleanly() {
        let mut group = Vec::new();
        prost::encoding::encode_key(999, prost::encoding::WireType::StartGroup, &mut group);
        prost::encoding::encode_key(1, prost::encoding::WireType::Varint, &mut group);
        prost::encoding::encode_varint(7, &mut group);
        prost::encoding::encode_key(999, prost::encoding::WireType::EndGroup, &mut group);

        let mut flood_after_group = group;
        flood_after_group.extend_from_slice(&len_field(7, &empty_elements(4, HOTSTUFF_MAX_DECODE_ITEMS + 5)));
        assert!(
            check_decode_budget::<proto::consensus::HotStuffMessage>(&flood_after_group, HOTSTUFF_MAX_DECODE_ITEMS)
                .is_err()
        );

        let malformed_samples: &[&[u8]] = &[
            &[0x00, 0x01],
            &[0x0e, 0x01],
            &[0x0f, 0x01],
            &[0x7c],
            &[0x7b, 0x08, 0x01],
            &[0x7b, 0x74],
            &[0x80],
            &[0x08, 0x80],
            &[0x0a, 0x7f, 0x01],
            &[0x79, 0x01],
            &[0x7d, 0x01],
        ];
        for sample in malformed_samples {
            assert_eq!(
                check_decode_budget::<proto::consensus::HotStuffMessage>(sample, 0).unwrap(),
                0
            );
            assert!(proto::consensus::HotStuffMessage::decode(*sample).is_err());
        }

        let mut node = RecursiveNode::default();
        for _ in 0..MAX_DEPTH {
            node = RecursiveNode {
                child: Some(Box::new(node)),
            };
        }
        assert_eq!(count::<RecursiveNode>(&node, usize::MAX).unwrap(), MAX_DEPTH);
        let deeper = RecursiveNode {
            child: Some(Box::new(node)),
        };
        assert!(count::<RecursiveNode>(&deeper, usize::MAX).is_err());
    }
}
