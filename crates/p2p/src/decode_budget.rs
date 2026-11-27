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

use tari_consensus::{
    consensus_constants::ConsensusConstants,
    messages::{MAX_CATCH_UP_PAGE_BLOCKS, MAX_REQUESTED_TRANSACTIONS},
};
use tari_ootle_common_types::shard::Shard;

use crate::proto;

/// Maximum embedded items permitted in a single direct consensus `HotStuffMessage` (other than `CatchUpPage`, which
/// scales its item budget to its multi-block page size).
///
/// Sized from `ConsensusConstants::MAINNET.max_commands_in_block` (`1,000` multi-shard commands with four nested
/// submessages each = `6,000` items, `BlockHeader` = `4`, `QuorumCertificate` with `1,000` signatures = `1,003`,
/// `TimeoutCertificate` with `1,000` signed timeouts = `2,003`, plus outer wrappers and foreign proposals < `10,300`
/// items; `MissingTransactionsRequest` and `MissingTransactionsResponse` at `1,000` entries use `1,001` items) with
/// more than 3x headroom while bounding worst-case empty-submessage amplification on a 4 MiB frame.
pub const HOTSTUFF_MAX_DECODE_ITEMS: usize =
    (ConsensusConstants::MAINNET.max_commands_in_block * 6 + tari_sidechain::MAX_QC_SIGNATURES * 3 + 512) * 3;

/// Wire-bytes divisor for the byte-proportional item allowance (`contents.len() / 8`) added on top of
/// [`HOTSTUFF_MAX_DECODE_ITEMS`] when walking a `CatchUpPage`.
///
/// An honest `CatchUpPage` is bounded by `DEFAULT_CATCH_UP_PAGE_BYTES = 3 MiB` and `MAX_CATCH_UP_PAGE_BLOCKS = 1,000`
/// blocks, plus up to `MAX_REQUESTED_TRANSACTIONS = 1,000` transactions. Because even a small certified block with one
/// `LocalOnly` command and a 7-signature QC carries ~16 items in ~600 bytes (about 1 item per 20–35 wire bytes),
/// `HotStuffMessage` grants `CatchUpPage` an additional byte-proportional allowance of `contents.len() / 8` items above
/// [`HOTSTUFF_MAX_DECODE_ITEMS`], while the `CatchUpPage` and `Block` walkers enforce per-field element caps.
const CATCH_UP_PAGE_BYTES_PER_EXTRA_ITEM: usize = 8;

/// Maximum `commands` entries permitted in a single `Block` on the wire.
const MAX_BLOCK_COMMANDS: usize = ConsensusConstants::MAINNET.max_commands_in_block;

/// Maximum `foreign_proposals` entries permitted in a single `ProposalMessage` on the wire.
const MAX_FOREIGN_PROPOSALS: usize = Shard::max().as_u32() as usize;

/// Deepest message nesting the walk follows. Schema walks only reach this on recursive types; exceeding it is treated
/// as over budget.
const MAX_DEPTH: usize = 128;

/// The payload would make `prost` allocate more embedded items than allowed, exceeds a per-field repeated element
/// limit, or nests deeper than [`MAX_DEPTH`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeBudgetExceeded {
    /// Items (or repeated field entries) counted when the walk stopped.
    pub items: usize,
    /// Configured item budget (or per-field limit).
    pub max: usize,
    /// Named repeated field when stopped by a per-field element cap rather than the total item budget.
    pub field: Option<&'static str>,
}

impl fmt::Display for DecodeBudgetExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.field {
            Some(field) => write!(
                f,
                "message exceeds the decode budget ({field} has {} entries, at most {} allowed)",
                self.items, self.max
            ),
            None => write!(
                f,
                "message exceeds the decode budget ({} embedded items, at most {} allowed)",
                self.items, self.max
            ),
        }
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
    fn new(max: usize) -> Self {
        Self {
            items: 0,
            max,
            depth: 0,
        }
    }

    /// Items counted so far.
    fn items(&self) -> usize {
        self.items
    }

    /// Extends the maximum allowed items by `extra`, used when a `CatchUpPage` scales its budget with its wire size.
    fn add_allowance(&mut self, extra: usize) {
        self.max = self.max.saturating_add(extra);
    }

    fn exceeded(&self) -> DecodeBudgetExceeded {
        DecodeBudgetExceeded {
            items: self.items,
            max: self.max,
            field: None,
        }
    }

    fn field_exceeded(field: &'static str, count: usize, max: usize) -> DecodeBudgetExceeded {
        DecodeBudgetExceeded {
            items: count,
            max,
            field: Some(field),
        }
    }

    /// Charges `count` leaf items that are not walked (e.g. elements of `repeated bytes` or the byte length of a
    /// packed repeated scalar field).
    fn charge_items(&mut self, count: usize) -> Result<(), DecodeBudgetExceeded> {
        self.items = self.items.saturating_add(count);
        if self.items > self.max {
            return Err(self.exceeded());
        }
        Ok(())
    }

    /// Charges one instance of message `T`, encoded in `contents`, and walks its fields.
    fn charge_message<T: DecodeBudget + ?Sized>(&mut self, contents: &[u8]) -> Result<(), DecodeBudgetExceeded> {
        self.items = self.items.saturating_add(1);
        if self.items > self.max || self.depth >= MAX_DEPTH {
            return Err(self.exceeded());
        }
        self.depth = self.depth.saturating_add(1);
        let result = T::count_messages(contents, self);
        self.depth = self.depth.saturating_sub(1);
        result
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
    // proto::common leaf messages reachable from HotStuffMessage
    proto::common::Signature,
    proto::common::SignatureAndPublicKey,
    // proto::transaction leaf messages reachable from HotStuffMessage
    proto::transaction::Transaction,
    // proto::consensus leaf messages reachable from HotStuffMessage
    proto::consensus::ForeignProposalRequestByBlockId,
    proto::consensus::ForeignProposal,
    proto::consensus::ExtraData,
    proto::consensus::LeaderFee,
    proto::consensus::EndEpochAtom,
    proto::consensus::ForeignProposalAtom,
    proto::consensus::LockedEpoch,
    proto::consensus::Decision,
    proto::consensus::Evidence,
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
fn walk_len_fields<F>(buf: &[u8], budget: &mut Budget, f: F) -> Result<(), DecodeBudgetExceeded>
where F: FnMut(u32, &[u8], &mut Budget) -> Result<(), DecodeBudgetExceeded> {
    walk_fields(buf, budget, &[], f)
}

/// Walks a message with repeated scalar fields under `repeated_scalar_tags`.
fn walk_fields<F>(
    buf: &[u8],
    budget: &mut Budget,
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

/// Tracks that a singular message field or `oneof` variant appears at most once in its parent message so that
/// `prost`'s merge-on-repeat semantics cannot concatenate repeated children across multiple occurrences of a singular
/// parent field.
fn visit_singular(seen: &mut usize, field: &'static str) -> Result<(), DecodeBudgetExceeded> {
    *seen = seen.saturating_add(1);
    if *seen > 1 {
        return Err(Budget::field_exceeded(field, *seen, 1));
    }
    Ok(())
}

impl DecodeBudget for proto::consensus::HotStuffMessage {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut message = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => {
                visit_singular(&mut message, "HotStuffMessage.message")?;
                budget.charge_message::<proto::consensus::NewViewMessage>(contents)
            },
            2 | 10 => {
                visit_singular(&mut message, "HotStuffMessage.message")?;
                budget.charge_message::<proto::consensus::ProposalMessage>(contents)
            },
            3 => {
                visit_singular(&mut message, "HotStuffMessage.message")?;
                budget.charge_message::<proto::consensus::ForeignProposalMessage>(contents)
            },
            4 => {
                visit_singular(&mut message, "HotStuffMessage.message")?;
                budget.charge_message::<proto::consensus::ForeignProposalNotification>(contents)
            },
            5 => {
                visit_singular(&mut message, "HotStuffMessage.message")?;
                budget.charge_message::<proto::consensus::ForeignProposalRequest>(contents)
            },
            6 => {
                visit_singular(&mut message, "HotStuffMessage.message")?;
                budget.charge_message::<proto::consensus::VoteMessage>(contents)
            },
            7 => {
                visit_singular(&mut message, "HotStuffMessage.message")?;
                budget.charge_message::<proto::consensus::MissingTransactionsRequest>(contents)
            },
            8 => {
                visit_singular(&mut message, "HotStuffMessage.message")?;
                budget.charge_message::<proto::consensus::MissingTransactionsResponse>(contents)
            },
            9 => {
                visit_singular(&mut message, "HotStuffMessage.message")?;
                budget.charge_message::<proto::consensus::SyncRequest>(contents)
            },
            11 => {
                visit_singular(&mut message, "HotStuffMessage.message")?;
                budget.add_allowance(contents.len() / CATCH_UP_PAGE_BYTES_PER_EXTRA_ITEM);
                budget.charge_message::<proto::consensus::CatchUpPage>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::CatchUpPage {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut blocks = 0usize;
        let mut leaf = 0usize;
        let mut transactions = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            3 => {
                blocks = blocks.saturating_add(1);
                if blocks > MAX_CATCH_UP_PAGE_BLOCKS {
                    return Err(Budget::field_exceeded(
                        "CatchUpPage.blocks",
                        blocks,
                        MAX_CATCH_UP_PAGE_BLOCKS,
                    ));
                }
                budget.charge_message::<proto::consensus::ProposalMessage>(contents)
            },
            4 => {
                visit_singular(&mut leaf, "CatchUpPage.leaf")?;
                budget.charge_message::<proto::consensus::ProposalMessage>(contents)
            },
            5 => {
                transactions = transactions.saturating_add(1);
                if transactions > MAX_REQUESTED_TRANSACTIONS {
                    return Err(Budget::field_exceeded(
                        "CatchUpPage.transactions",
                        transactions,
                        MAX_REQUESTED_TRANSACTIONS,
                    ));
                }
                budget.charge_message::<proto::transaction::Transaction>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::NewViewMessage {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut high_qc = 0usize;
        let mut last_vote = 0usize;
        let mut timeout = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => {
                visit_singular(&mut high_qc, "NewViewMessage.high_qc")?;
                budget.charge_message::<proto::consensus::QuorumCertificate>(contents)
            },
            2 => {
                visit_singular(&mut last_vote, "NewViewMessage.last_vote")?;
                budget.charge_message::<proto::consensus::VoteMessage>(contents)
            },
            3 => {
                visit_singular(&mut timeout, "NewViewMessage.timeout")?;
                budget.charge_message::<proto::consensus::TimeoutVote>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::ProposalMessage {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut block = 0usize;
        let mut foreign_proposals = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => {
                visit_singular(&mut block, "ProposalMessage.block")?;
                budget.charge_message::<proto::consensus::Block>(contents)
            },
            2 => {
                foreign_proposals = foreign_proposals.saturating_add(1);
                if foreign_proposals > MAX_FOREIGN_PROPOSALS {
                    return Err(Budget::field_exceeded(
                        "ProposalMessage.foreign_proposals",
                        foreign_proposals,
                        MAX_FOREIGN_PROPOSALS,
                    ));
                }
                budget.charge_message::<proto::consensus::ForeignProposal>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::TimeoutVote {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut signature = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            3 => {
                visit_singular(&mut signature, "TimeoutVote.signature")?;
                budget.charge_message::<proto::common::SignatureAndPublicKey>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::ForeignProposalMessage {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut proposal = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => {
                visit_singular(&mut proposal, "ForeignProposalMessage.proposal")?;
                budget.charge_message::<proto::consensus::ForeignProposal>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::ForeignProposalNotification {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        walk_fields(buf, budget, &[3], |tag, contents, budget| match tag {
            3 => budget.charge_items(contents.len()),
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::ForeignProposalRequest {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut request = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => {
                visit_singular(&mut request, "ForeignProposalRequest.request")?;
                budget.charge_message::<proto::consensus::ForeignProposalRequestByBlockId>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::VoteMessage {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut signature = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            5 => {
                visit_singular(&mut signature, "VoteMessage.signature")?;
                budget.charge_message::<proto::common::SignatureAndPublicKey>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::BlockHeader {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut signature = 0usize;
        let mut extra_data = 0usize;
        let mut accumulated_data = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            10 => {
                visit_singular(&mut signature, "BlockHeader.signature")?;
                budget.charge_message::<proto::common::Signature>(contents)
            },
            13 => {
                visit_singular(&mut extra_data, "BlockHeader.extra_data")?;
                budget.charge_message::<proto::consensus::ExtraData>(contents)
            },
            14 => {
                visit_singular(&mut accumulated_data, "BlockHeader.accumulated_data")?;
                budget.charge_message::<proto::consensus::ShardGroupAccumulatedData>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::Block {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut header = 0usize;
        let mut justify = 0usize;
        let mut commands = 0usize;
        let mut timeout_certificate = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => {
                visit_singular(&mut header, "Block.header")?;
                budget.charge_message::<proto::consensus::BlockHeader>(contents)
            },
            2 => {
                visit_singular(&mut justify, "Block.justify")?;
                budget.charge_message::<proto::consensus::QuorumCertificate>(contents)
            },
            3 => {
                commands = commands.saturating_add(1);
                if commands > MAX_BLOCK_COMMANDS {
                    return Err(Budget::field_exceeded("Block.commands", commands, MAX_BLOCK_COMMANDS));
                }
                budget.charge_message::<proto::consensus::Command>(contents)
            },
            4 => {
                visit_singular(&mut timeout_certificate, "Block.timeout_certificate")?;
                budget.charge_message::<proto::consensus::TimeoutCertificate>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::Command {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut command = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => {
                visit_singular(&mut command, "Command.command")?;
                budget.charge_message::<proto::consensus::LocalOnlyAtom>(contents)
            },
            2..=5 => {
                visit_singular(&mut command, "Command.command")?;
                budget.charge_message::<proto::consensus::MultiShardAtom>(contents)
            },
            6 => {
                visit_singular(&mut command, "Command.command")?;
                budget.charge_message::<proto::consensus::ForeignProposalAtom>(contents)
            },
            7 => {
                visit_singular(&mut command, "Command.command")?;
                budget.charge_message::<proto::consensus::EndEpochAtom>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::MultiShardAtom {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut decision = 0usize;
        let mut evidence = 0usize;
        let mut leader_fee = 0usize;
        let mut locked_epoch = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            3 => {
                visit_singular(&mut decision, "MultiShardAtom.decision")?;
                budget.charge_message::<proto::consensus::Decision>(contents)
            },
            4 => {
                visit_singular(&mut evidence, "MultiShardAtom.evidence")?;
                budget.charge_message::<proto::consensus::Evidence>(contents)
            },
            6 => {
                visit_singular(&mut leader_fee, "MultiShardAtom.leader_fee")?;
                budget.charge_message::<proto::consensus::LeaderFee>(contents)
            },
            8 => {
                visit_singular(&mut locked_epoch, "MultiShardAtom.locked_epoch")?;
                budget.charge_message::<proto::consensus::LockedEpoch>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::LocalOnlyAtom {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut decision = 0usize;
        let mut leader_fee = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            2 => {
                visit_singular(&mut decision, "LocalOnlyAtom.decision")?;
                budget.charge_message::<proto::consensus::Decision>(contents)
            },
            4 => {
                visit_singular(&mut leader_fee, "LocalOnlyAtom.leader_fee")?;
                budget.charge_message::<proto::consensus::LeaderFee>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::QuorumCertificate {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut signatures = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            9 => {
                visit_singular(&mut signatures, "QuorumCertificate.signatures")?;
                budget.charge_message::<proto::consensus::QcSignatures>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::QcSignatures {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut scheme = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => {
                visit_singular(&mut scheme, "QcSignatures.scheme")?;
                budget.charge_message::<proto::consensus::SchnorrQcSignatures>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::SchnorrQcSignatures {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut signatures = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            2 => {
                signatures = signatures.saturating_add(1);
                if signatures > tari_sidechain::MAX_QC_SIGNATURES {
                    return Err(Budget::field_exceeded(
                        "SchnorrQcSignatures.signatures",
                        signatures,
                        tari_sidechain::MAX_QC_SIGNATURES,
                    ));
                }
                budget.charge_message::<proto::common::Signature>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::TimeoutCertificate {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut signatures = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            3 => {
                visit_singular(&mut signatures, "TimeoutCertificate.signatures")?;
                budget.charge_message::<proto::consensus::TcSignatures>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::TcSignatures {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut scheme = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            1 => {
                visit_singular(&mut scheme, "TcSignatures.scheme")?;
                budget.charge_message::<proto::consensus::SchnorrTcSignatures>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::SchnorrTcSignatures {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut timeouts = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            2 => {
                timeouts = timeouts.saturating_add(1);
                if timeouts > tari_sidechain::MAX_QC_SIGNATURES {
                    return Err(Budget::field_exceeded(
                        "SchnorrTcSignatures.timeouts",
                        timeouts,
                        tari_sidechain::MAX_QC_SIGNATURES,
                    ));
                }
                budget.charge_message::<proto::consensus::SignedTimeout>(contents)
            },
            _ => Ok(()),
        })
    }
}

impl DecodeBudget for proto::consensus::SignedTimeout {
    fn count_messages(buf: &[u8], budget: &mut Budget) -> Result<(), DecodeBudgetExceeded> {
        let mut signature = 0usize;
        walk_len_fields(buf, budget, |tag, contents, budget| match tag {
            2 => {
                visit_singular(&mut signature, "SignedTimeout.signature")?;
                budget.charge_message::<proto::common::Signature>(contents)
            },
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

const _: fn(proto::consensus::HotStuffMessage) = assert_hotstuff_schema_exhaustive;

/// Compile-time destructuring witness over `HotStuffMessage` and its nested types: adding a field or `oneof` variant
/// in `consensus.proto`, `common.proto`, or `transaction.proto` fails to compile here so the author is prompted to
/// review the [`DecodeBudget`] walkers above.
fn assert_hotstuff_schema_exhaustive(msg: proto::consensus::HotStuffMessage) {
    use proto::consensus::{
        CatchUpPage,
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
        HotStuffKind::ForeignProposalRequest(ForeignProposalRequest { request }) => match request {
            Some(ForeignRequestKind::ByBlockId(ForeignProposalRequestByBlockId {
                block_id: _,
                for_shard_group: _,
                epoch: _,
            })) |
            None => {},
        },
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
            from_height: _,
            request_id: _,
            skip: _,
        }) => {},
        HotStuffKind::CatchUpPage(CatchUpPage {
            request_id: _,
            epoch: _,
            blocks,
            leaf,
            transactions,
            next_height: _,
            next_skip: _,
        }) => {
            for block in blocks {
                witness_proposal(block);
            }
            if let Some(leaf) = leaf {
                witness_proposal(leaf);
            }
            for proto::transaction::Transaction { bor_encoded: _ } in transactions {}
        },
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
    if let Some(QcSignatures { scheme }) = signatures {
        match scheme {
            Some(Scheme::Schnorr(SchnorrQcSignatures { signers: _, signatures })) => {
                for sig in signatures {
                    witness_sig(sig);
                }
            },
            None => {},
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
    if let Some(TcSignatures { scheme }) = signatures {
        match scheme {
            Some(Scheme::Schnorr(SchnorrTcSignatures { signers: _, timeouts })) => {
                for SignedTimeout {
                    high_pc_height: _,
                    signature,
                } in timeouts
                {
                    if let Some(sig) = signature {
                        witness_sig(sig);
                    }
                }
            },
            None => {},
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
    match decision {
        Some(Kind::Commit(_) | Kind::Abort(_)) | None => {},
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
    use tari_consensus::hotstuff::DEFAULT_CATCH_UP_PAGE_BYTES;
    use tari_ootle_common_types::ShardGroup;
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

    fn populated_local_only_command() -> proto::consensus::Command {
        proto::consensus::Command {
            command: Some(proto::consensus::command::Command::LocalOnly(
                proto::consensus::LocalOnlyAtom {
                    id: vec![0x11; 32],
                    decision: Some(proto::consensus::Decision {
                        decision: Some(proto::consensus::decision::Decision::Commit(true)),
                    }),
                    fee: 100,
                    leader_fee: Some(proto::consensus::LeaderFee {
                        leader_fee: 90,
                        exhaust_burn: 10,
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

    fn populated_header() -> proto::consensus::BlockHeader {
        proto::consensus::BlockHeader {
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
                from_height: 42,
                request_id: 7,
                skip: 0,
            })),
        };
        assert_eq!(count::<proto::consensus::HotStuffMessage>(&sync_req, 10).unwrap(), 1);
    }

    #[test]
    fn admits_maximally_populated_valid_proposal_with_headroom() {
        let max_sigs = tari_sidechain::MAX_QC_SIGNATURES;

        for constants in [
            ConsensusConstants::mainnet(),
            ConsensusConstants::devnet(7),
            ConsensusConstants::esmeralda(),
            ConsensusConstants::testnet(),
        ] {
            let proposal = proto::consensus::ProposalMessage {
                block: Some(proto::consensus::Block {
                    header: Some(populated_header()),
                    justify: Some(populated_qc(max_sigs)),
                    commands: vec![populated_multi_shard_command(); constants.max_commands_in_block],
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
    }

    #[test]
    fn admits_maximally_populated_catch_up_pages() {
        // 1. 1,000 blocks plus leaf and 1,000 transactions within DEFAULT_CATCH_UP_PAGE_BYTES (3 MiB).
        let small_block = proto::consensus::ProposalMessage {
            block: Some(proto::consensus::Block {
                header: Some(populated_header()),
                justify: Some(populated_qc(5)),
                commands: vec![populated_local_only_command(); 4],
                timeout_certificate: None,
            }),
            foreign_proposals: vec![],
        };
        let thousand_block_page = proto::consensus::HotStuffMessage {
            message: Some(proto::consensus::hot_stuff_message::Message::CatchUpPage(
                proto::consensus::CatchUpPage {
                    request_id: 1,
                    epoch: 5,
                    blocks: vec![small_block.clone(); MAX_CATCH_UP_PAGE_BLOCKS],
                    leaf: Some(small_block),
                    transactions: vec![
                        proto::transaction::Transaction {
                            bor_encoded: vec![0xaa; 128],
                        };
                        MAX_REQUESTED_TRANSACTIONS
                    ],
                    next_height: Some(1001),
                    next_skip: 0,
                },
            )),
        };
        assert!(thousand_block_page.encoded_len() <= DEFAULT_CATCH_UP_PAGE_BYTES);
        assert!(count::<proto::consensus::HotStuffMessage>(&thousand_block_page, HOTSTUFF_MAX_DECODE_ITEMS).is_ok());

        // 2. Dense 3 MiB page filled with blocks packed with compact LocalOnly commands.
        let dense_block = proto::consensus::ProposalMessage {
            block: Some(proto::consensus::Block {
                header: Some(populated_header()),
                justify: Some(populated_qc(5)),
                commands: vec![populated_local_only_command(); 500],
                timeout_certificate: None,
            }),
            foreign_proposals: vec![],
        };
        let dense_block_len = dense_block.encoded_len();
        let dense_count = (DEFAULT_CATCH_UP_PAGE_BYTES / dense_block_len).min(MAX_CATCH_UP_PAGE_BLOCKS);
        let dense_page = proto::consensus::HotStuffMessage {
            message: Some(proto::consensus::hot_stuff_message::Message::CatchUpPage(
                proto::consensus::CatchUpPage {
                    request_id: 2,
                    epoch: 5,
                    blocks: vec![dense_block; dense_count],
                    leaf: None,
                    transactions: vec![],
                    next_height: Some(dense_count as u64 + 1),
                    next_skip: 0,
                },
            )),
        };
        assert!(dense_page.encoded_len() <= DEFAULT_CATCH_UP_PAGE_BYTES);
        assert!(count::<proto::consensus::HotStuffMessage>(&dense_page, HOTSTUFF_MAX_DECODE_ITEMS).is_ok());
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
        let packed_items = count::<proto::consensus::HotStuffMessage>(&msg, HOTSTUFF_MAX_DECODE_ITEMS).unwrap();
        assert!(packed_items <= HOTSTUFF_MAX_DECODE_ITEMS);

        // Unpacked repeated uint32 elements are charged one item per element.
        let mut unpacked_notif = len_field(1, &[0xab; 32]);
        for sg in &shard_groups {
            prost::encoding::encode_key(3, prost::encoding::WireType::Varint, &mut unpacked_notif);
            prost::encoding::encode_varint(u64::from(*sg), &mut unpacked_notif);
        }
        let outer = len_field(4, &unpacked_notif);
        let unpacked_items =
            check_decode_budget::<proto::consensus::HotStuffMessage>(&outer, HOTSTUFF_MAX_DECODE_ITEMS).unwrap();
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
            field: None,
        });

        // MissingTransactionsResponse (tag 8) -> repeated Transaction (tag 4)
        let tx_flood = len_field(8, &empty_elements(4, flood_count));
        assert!(
            check_decode_budget::<proto::consensus::HotStuffMessage>(&tx_flood, HOTSTUFF_MAX_DECODE_ITEMS).is_err()
        );

        // CatchUpPage (tag 11) -> repeated ProposalMessage blocks (tag 3) and repeated Transaction (tag 5),
        // including 8-byte padded entries matching CATCH_UP_PAGE_BYTES_PER_EXTRA_ITEM (1a 06 + three 18 00 varints).
        let padded_entry_body = [0x18, 0x00, 0x18, 0x00, 0x18, 0x00];
        let padded_block_entry = len_field(3, &padded_entry_body);
        assert_eq!(padded_block_entry.len(), CATCH_UP_PAGE_BYTES_PER_EXTRA_ITEM);
        let max_padded_blocks = len_field(11, &padded_block_entry.repeat(MAX_CATCH_UP_PAGE_BLOCKS));
        assert!(
            check_decode_budget::<proto::consensus::HotStuffMessage>(&max_padded_blocks, HOTSTUFF_MAX_DECODE_ITEMS)
                .is_ok()
        );
        let padded_blocks_flood = len_field(11, &padded_block_entry.repeat(MAX_CATCH_UP_PAGE_BLOCKS + 1));
        let err =
            check_decode_budget::<proto::consensus::HotStuffMessage>(&padded_blocks_flood, HOTSTUFF_MAX_DECODE_ITEMS)
                .unwrap_err();
        assert_eq!(err.field, Some("CatchUpPage.blocks"));
        let padded_tx_entry = len_field(5, &padded_entry_body);
        assert_eq!(padded_tx_entry.len(), CATCH_UP_PAGE_BYTES_PER_EXTRA_ITEM);
        let padded_tx_flood = len_field(11, &padded_tx_entry.repeat(MAX_REQUESTED_TRANSACTIONS + 1));
        let err = check_decode_budget::<proto::consensus::HotStuffMessage>(&padded_tx_flood, HOTSTUFF_MAX_DECODE_ITEMS)
            .unwrap_err();
        assert_eq!(err.field, Some("CatchUpPage.transactions"));
        let padded_cmd_entry = len_field(3, &padded_entry_body);
        let overfull_block_in_page = len_field(
            11,
            &len_field(
                3,
                &len_field(1, &padded_cmd_entry.repeat(MAX_BLOCK_COMMANDS + 1)),
            ),
        );
        let err = check_decode_budget::<proto::consensus::HotStuffMessage>(
            &overfull_block_in_page,
            HOTSTUFF_MAX_DECODE_ITEMS,
        )
        .unwrap_err();
        assert_eq!(err.field, Some("Block.commands"));

        // Splitting a page across repeated HotStuffMessage.catch_up_page (tag 11) fields or a block across repeated
        // ProposalMessage.block (tag 1) fields (which prost would merge into one Vec) is rejected at the second
        // singular occurrence.
        let half_page = len_field(11, &padded_block_entry.repeat(MAX_CATCH_UP_PAGE_BLOCKS / 2 + 1));
        let split_pages = [half_page.as_slice(), half_page.as_slice()].concat();
        let err = check_decode_budget::<proto::consensus::HotStuffMessage>(&split_pages, HOTSTUFF_MAX_DECODE_ITEMS)
            .unwrap_err();
        assert_eq!(err.field, Some("HotStuffMessage.message"));

        let half_block = len_field(1, &padded_cmd_entry.repeat(MAX_BLOCK_COMMANDS / 2 + 1));
        let split_blocks_proposal = len_field(2, &[half_block.as_slice(), half_block.as_slice()].concat());
        let err = check_decode_budget::<proto::consensus::HotStuffMessage>(
            &split_blocks_proposal,
            HOTSTUFF_MAX_DECODE_ITEMS,
        )
        .unwrap_err();
        assert_eq!(err.field, Some("ProposalMessage.block"));

        // Padded QC signatures, TC timeouts, and foreign proposals inside a CatchUpPage block are capped at their
        // consensus limits rather than growing to the page's per-byte item allowance.
        let padded_sig_entry = len_field(2, &padded_entry_body);
        let overfull_qc_in_page = len_field(
            11,
            &len_field(
                3,
                &len_field(
                    1,
                    &len_field(
                        2,
                        &len_field(
                            9,
                            &len_field(
                                1,
                                &padded_sig_entry.repeat(tari_sidechain::MAX_QC_SIGNATURES + 1),
                            ),
                        ),
                    ),
                ),
            ),
        );
        let err = check_decode_budget::<proto::consensus::HotStuffMessage>(
            &overfull_qc_in_page,
            HOTSTUFF_MAX_DECODE_ITEMS,
        )
        .unwrap_err();
        assert_eq!(err.field, Some("SchnorrQcSignatures.signatures"));

        let overfull_tc_in_page = len_field(
            11,
            &len_field(
                3,
                &len_field(
                    1,
                    &len_field(
                        4,
                        &len_field(
                            3,
                            &len_field(
                                1,
                                &padded_sig_entry.repeat(tari_sidechain::MAX_QC_SIGNATURES + 1),
                            ),
                        ),
                    ),
                ),
            ),
        );
        let err = check_decode_budget::<proto::consensus::HotStuffMessage>(
            &overfull_tc_in_page,
            HOTSTUFF_MAX_DECODE_ITEMS,
        )
        .unwrap_err();
        assert_eq!(err.field, Some("SchnorrTcSignatures.timeouts"));

        let padded_fp_entry = len_field(2, &padded_entry_body);
        let overfull_fp_in_page = len_field(
            11,
            &len_field(3, &padded_fp_entry.repeat(MAX_FOREIGN_PROPOSALS + 1)),
        );
        let err = check_decode_budget::<proto::consensus::HotStuffMessage>(
            &overfull_fp_in_page,
            HOTSTUFF_MAX_DECODE_ITEMS,
        )
        .unwrap_err();
        assert_eq!(err.field, Some("ProposalMessage.foreign_proposals"));

        // NewView (tag 1) -> QuorumCertificate (tag 1) -> QcSignatures (tag 9) -> Schnorr (tag 1) -> Signature (tag 2)
        let qc_sig_flood = len_field(
            1,
            &len_field(1, &len_field(9, &len_field(1, &empty_elements(2, flood_count)))),
        );
        assert!(
            check_decode_budget::<proto::consensus::HotStuffMessage>(&qc_sig_flood, HOTSTUFF_MAX_DECODE_ITEMS).is_err()
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
