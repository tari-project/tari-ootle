//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Rollback-audit file format.
//!
//! Binary, length-prefixed stream of borsh-encoded `AuditRecord` values, written by
//! `tari_validator_rollback` when it performs (or dry-runs) a rollback. One audit file
//! per validator per invocation. The file is both a human-inspectable breadcrumb (via
//! the tool's `inspect` subcommand) and a machine-readable input for downstream
//! tooling (via `convert`).
//!
//! ## Field representation
//!
//! Domain ID types (`BlockId`, `SubstateId`, `TransactionId`) are stored as the
//! `Display`/`FromStr` round-trip — the human-readable form (`"component_abc…"`,
//! `"block_f00…"`) — because (a) borsh can't derive on those types upstream, (b) hex
//! strings survive a JSON round-trip without data loss, and (c) operators grepping the
//! converted JSONL get matchable strings out of the box. The wire size cost is
//! acceptable for an audit artifact.

use std::{
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
};

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

/// ASCII "TARR" magic. Distinguishes rollback-audit files from any other binary
/// rubbish an operator might point the tool at.
pub const MAGIC: u32 = 0x5441_5252;

/// File format version. Increment whenever the record schema changes in a
/// non-backward-compatible way.
pub const FORMAT_VERSION: u8 = 1;

#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Malformed audit file: {0}")]
    Malformed(String),
    #[error("Unsupported audit format version {0} (this tool expects {FORMAT_VERSION})")]
    UnsupportedVersion(u8),
    #[error("Record decode error: {0}")]
    Decode(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// A single entry in the audit stream.
#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuditRecord {
    Header(AuditHeader),
    SubstateSummary(SubstateSummary),
    SubstateTransition(SubstateTransition),
    TransactionUnfinalised(TransactionUnfinalised),
    Footer(AuditFooter),
}

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct AuditHeader {
    pub target_epoch: u64,
    pub shard_group: AuditShardGroup,
    pub pre_rollback_tip_epoch: Option<u64>,
    pub pre_rollback_tip_block: Option<String>,
    pub state_version_per_shard: Vec<(AuditShard, u64)>,
    pub generated_at_unix_secs: u64,
    pub tool_version: String,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, BorshSerialize, BorshDeserialize, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditShardGroup {
    pub start: u32,
    pub end_inclusive: u32,
}

/// Serialisable wrapper for `Shard` with a sentinel for the global shard.
#[derive(Debug, Clone, Copy, BorshSerialize, BorshDeserialize, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuditShard {
    Global,
    Numbered(u32),
}

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct SubstateSummary {
    pub substate_id: String,
    pub shard: AuditShard,
    pub action: SubstateAction,
    pub pre_rollback_version: u32,
    /// `None` when `action == Removed`.
    pub post_rollback_version: Option<u32>,
}

#[derive(Debug, Clone, Copy, BorshSerialize, BorshDeserialize, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubstateAction {
    /// Substate will cease to exist after the rollback — it was first created at a
    /// version > checkpoint.
    Removed,
    /// Substate existed at checkpoint and had further transitions after. Rolls back
    /// to its pre-rollback version.
    Rewound,
}

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct SubstateTransition {
    pub substate_id: String,
    pub shard: AuditShard,
    pub state_version: u64,
    pub transition: TransitionKind,
    pub epoch: u64,
}

#[derive(Debug, Clone, Copy, BorshSerialize, BorshDeserialize, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    /// Original transition was `Up`; rollback deletes the created substate.
    Up,
    /// Original transition was `Down`; rollback restores the destroyed substate.
    Down,
}

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct TransactionUnfinalised {
    pub transaction_id: String,
    pub finalised_in_block: String,
    pub finalised_at_epoch: u64,
}

#[derive(Debug, Clone, Copy, Default, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct AuditFooter {
    pub substates_removed: u64,
    pub substates_rewound: u64,
    pub substate_transitions: u64,
    pub transactions_unfinalised: u64,
    pub blocks_deleted: u64,
}

/// Streaming writer: writes magic + version header on construction, then one
/// length-prefixed borsh record per `write_record` call.
pub struct AuditWriter<W: Write> {
    inner: W,
}

impl AuditWriter<BufWriter<File>> {
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self, AuditError> {
        let file = File::create(path)?;
        Self::new(BufWriter::new(file))
    }
}

impl<W: Write> AuditWriter<W> {
    pub fn new(mut inner: W) -> Result<Self, AuditError> {
        inner.write_all(&MAGIC.to_le_bytes())?;
        inner.write_all(&[FORMAT_VERSION, 0u8, 0u8, 0u8])?;
        Ok(Self { inner })
    }

    pub fn write_record(&mut self, record: &AuditRecord) -> Result<(), AuditError> {
        let mut buf = Vec::new();
        BorshSerialize::serialize(record, &mut buf).map_err(|e| AuditError::Decode(e.to_string()))?;
        let len: u32 = buf
            .len()
            .try_into()
            .map_err(|_| AuditError::Malformed("record exceeds u32 length".into()))?;
        self.inner.write_all(&len.to_le_bytes())?;
        self.inner.write_all(&buf)?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), AuditError> {
        self.inner.flush()?;
        Ok(())
    }
}

/// Streaming reader: validates the magic + version on open and yields one record
/// per `read_record` call. Returns `Ok(None)` on EOF.
#[derive(Debug)]
pub struct AuditReader<R: Read> {
    inner: R,
}

impl AuditReader<BufReader<File>> {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, AuditError> {
        let file = File::open(path)?;
        Self::new(BufReader::new(file))
    }
}

impl<R: Read> AuditReader<R> {
    pub fn new(mut inner: R) -> Result<Self, AuditError> {
        let mut magic = [0u8; 4];
        inner.read_exact(&mut magic)?;
        if u32::from_le_bytes(magic) != MAGIC {
            return Err(AuditError::Malformed("bad magic — not a rollback audit file".into()));
        }
        let mut header = [0u8; 4];
        inner.read_exact(&mut header)?;
        let version = header[0];
        if version != FORMAT_VERSION {
            return Err(AuditError::UnsupportedVersion(version));
        }
        Ok(Self { inner })
    }

    pub fn read_record(&mut self) -> Result<Option<AuditRecord>, AuditError> {
        let mut len_buf = [0u8; 4];
        let got = match self.inner.read(&mut len_buf)? {
            0 => return Ok(None),
            n => n,
        };
        // A partial first read is possible on some streams — top up to 4 bytes.
        let mut total = got;
        while total < 4 {
            let n = self.inner.read(&mut len_buf[total..])?;
            if n == 0 {
                return Err(AuditError::Malformed("truncated record-length prefix".into()));
            }
            total += n;
        }
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        self.inner.read_exact(&mut buf)?;
        let mut cursor = &buf[..];
        let record =
            BorshDeserialize::deserialize_reader(&mut cursor).map_err(|e| AuditError::Decode(e.to_string()))?;
        Ok(Some(record))
    }

    pub fn records(self) -> AuditRecordIter<R> {
        AuditRecordIter { reader: self }
    }
}

pub struct AuditRecordIter<R: Read> {
    reader: AuditReader<R>,
}

impl<R: Read> Iterator for AuditRecordIter<R> {
    type Item = Result<AuditRecord, AuditError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.reader.read_record() {
            Ok(Some(r)) => Some(Ok(r)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_single_footer() {
        let mut buf = Vec::new();
        let mut writer = AuditWriter::new(&mut buf).unwrap();
        writer
            .write_record(&AuditRecord::Footer(AuditFooter {
                substates_removed: 10,
                substates_rewound: 20,
                substate_transitions: 30,
                transactions_unfinalised: 40,
                blocks_deleted: 50,
            }))
            .unwrap();
        writer.finish().unwrap();

        let mut reader = AuditReader::new(buf.as_slice()).unwrap();
        let rec = reader.read_record().unwrap().unwrap();
        match rec {
            AuditRecord::Footer(f) => {
                assert_eq!(f.substates_removed, 10);
                assert_eq!(f.blocks_deleted, 50);
            },
            other => panic!("unexpected record {other:?}"),
        }
        assert!(reader.read_record().unwrap().is_none());
    }

    #[test]
    fn rejects_bad_magic() {
        let bytes = [0u8; 8];
        let err = AuditReader::new(bytes.as_slice()).unwrap_err();
        assert!(matches!(err, AuditError::Malformed(_)));
    }

    #[test]
    fn rejects_unsupported_version() {
        let mut bytes = MAGIC.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[99, 0, 0, 0]);
        let err = AuditReader::new(bytes.as_slice()).unwrap_err();
        assert!(matches!(err, AuditError::UnsupportedVersion(99)));
    }

    #[test]
    fn streams_multiple_records() {
        let mut buf = Vec::new();
        let mut writer = AuditWriter::new(&mut buf).unwrap();
        writer
            .write_record(&AuditRecord::Header(AuditHeader {
                target_epoch: 42,
                shard_group: AuditShardGroup {
                    start: 0,
                    end_inclusive: 63,
                },
                pre_rollback_tip_epoch: Some(47),
                pre_rollback_tip_block: Some("block_dead".into()),
                state_version_per_shard: vec![(AuditShard::Global, 100), (AuditShard::Numbered(3), 200)],
                generated_at_unix_secs: 123,
                tool_version: "0.1.0".into(),
                dry_run: false,
            }))
            .unwrap();
        writer
            .write_record(&AuditRecord::Footer(AuditFooter::default()))
            .unwrap();
        writer.finish().unwrap();

        let reader = AuditReader::new(buf.as_slice()).unwrap();
        let records: Vec<_> = reader.records().collect::<Result<_, _>>().unwrap();
        assert_eq!(records.len(), 2);
        assert!(matches!(records[0], AuditRecord::Header(_)));
        assert!(matches!(records[1], AuditRecord::Footer(_)));
    }
}
