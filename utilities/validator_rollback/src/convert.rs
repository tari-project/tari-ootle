//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    ffi::OsString,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::Context;
use clap::{ArgEnum, Args as ClapArgs};

use crate::audit::{AuditReader, AuditRecord};

#[derive(Debug, ClapArgs)]
pub struct Args {
    #[clap(long)]
    audit: PathBuf,

    #[clap(long, arg_enum)]
    format: Format,

    /// Output path. If omitted, writes to stdout.
    #[clap(long)]
    out: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ArgEnum)]
pub enum Format {
    /// One JSON object per line, `kind` field discriminating variants.
    Jsonl,
    /// Single JSON document — `header`, `summaries`, `transitions`, `transactions_unfinalised`, `footer`.
    Json,
}

struct TempFileGuard {
    path: PathBuf,
    armed: bool,
}

impl TempFileGuard {
    fn new(target: &Path) -> Self {
        let mut name = target.file_name().map(OsString::from).unwrap_or_default();
        name.push(format!(".tmp.{}", std::process::id()));
        Self {
            path: target.with_file_name(name),
            armed: true,
        }
    }

    fn commit(mut self, target: &Path) -> anyhow::Result<()> {
        std::fs::rename(&self.path, target).with_context(|| format!("moving {:?} to {:?}", self.path, target))?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if self.armed {
            let _unused = std::fs::remove_file(&self.path);
        }
    }
}

pub fn run(args: Args) -> anyhow::Result<()> {
    let reader = AuditReader::open(&args.audit).with_context(|| format!("opening {:?}", args.audit))?;

    match args.out.as_ref() {
        Some(path) => {
            let temp = TempFileGuard::new(path);
            {
                let file = std::fs::File::create(&temp.path).with_context(|| format!("creating {:?}", temp.path))?;
                let mut out = std::io::BufWriter::new(file);
                convert_records(reader, args.format, &mut out)?;
                out.flush()?;
            }
            temp.commit(path)?;
        },
        None => {
            let mut out = std::io::stdout().lock();
            convert_records(reader, args.format, &mut out)?;
            out.flush()?;
        },
    }
    Ok(())
}

fn validate_boundary(record: &AuditRecord, header_seen: &mut bool, footer_seen: &mut bool) -> anyhow::Result<()> {
    match record {
        AuditRecord::Header(_) => {
            if *header_seen {
                anyhow::bail!("audit file contains multiple header records");
            }
            *header_seen = true;
        },
        AuditRecord::Footer(_) => {
            if *footer_seen {
                anyhow::bail!("audit file contains multiple footer records");
            }
            *footer_seen = true;
        },
        _ => {},
    }
    Ok(())
}

fn ensure_complete(header_seen: bool, footer_seen: bool) -> anyhow::Result<()> {
    if !header_seen {
        anyhow::bail!("audit file is missing its header record");
    }
    if !footer_seen {
        anyhow::bail!("audit file is missing its footer record (truncated?)");
    }
    Ok(())
}

fn convert_records<R: Read, W: Write>(reader: AuditReader<R>, format: Format, mut out: W) -> anyhow::Result<()> {
    let mut header_seen = false;
    let mut footer_seen = false;

    match format {
        Format::Jsonl => {
            for record in reader.records() {
                let record = record?;
                validate_boundary(&record, &mut header_seen, &mut footer_seen)?;
                serde_json::to_writer(&mut out, &record)?;
                writeln!(out)?;
            }
            ensure_complete(header_seen, footer_seen)?;
        },
        Format::Json => {
            let mut header = serde_json::Value::Null;
            let mut summaries = Vec::new();
            let mut transitions = Vec::new();
            let mut transactions_unfinalised = Vec::new();
            let mut footer = serde_json::Value::Null;
            for record in reader.records() {
                let record = record?;
                validate_boundary(&record, &mut header_seen, &mut footer_seen)?;
                match record {
                    AuditRecord::Header(h) => header = serde_json::to_value(h)?,
                    AuditRecord::SubstateSummary(s) => summaries.push(serde_json::to_value(s)?),
                    AuditRecord::SubstateTransition(t) => transitions.push(serde_json::to_value(t)?),
                    AuditRecord::TransactionUnfinalised(t) => transactions_unfinalised.push(serde_json::to_value(t)?),
                    AuditRecord::Footer(f) => footer = serde_json::to_value(f)?,
                }
            }
            ensure_complete(header_seen, footer_seen)?;
            let doc = serde_json::json!({
                "header": header,
                "summaries": summaries,
                "transitions": transitions,
                "transactions_unfinalised": transactions_unfinalised,
                "footer": footer,
            });
            serde_json::to_writer_pretty(&mut out, &doc)?;
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{AuditFooter, AuditHeader, AuditShard, AuditShardGroup, AuditWriter};

    fn sample_header() -> AuditRecord {
        AuditRecord::Header(AuditHeader {
            target_epoch: 10,
            shard_group: AuditShardGroup {
                start: 0,
                end_inclusive: 63,
            },
            pre_rollback_tip_epoch: Some(12),
            pre_rollback_tip_block: Some("block_1234".into()),
            state_version_per_shard: vec![(AuditShard::Global, 5)],
            generated_at_unix_secs: 1_700_000_000,
            tool_version: "0.1.0".into(),
            dry_run: true,
        })
    }

    fn sample_footer() -> AuditRecord {
        AuditRecord::Footer(AuditFooter {
            substates_removed: 1,
            substates_rewound: 2,
            substate_transitions: 3,
            transactions_unfinalised: 4,
            blocks_deleted: 5,
        })
    }

    fn encode_records(records: &[AuditRecord]) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut writer = AuditWriter::new(&mut buf).unwrap();
        for record in records {
            writer.write_record(record).unwrap();
        }
        writer.finish().unwrap();
        buf
    }

    #[test]
    fn converts_valid_audit_to_json_and_jsonl() {
        let bytes = encode_records(&[sample_header(), sample_footer()]);

        let mut json_out = Vec::new();
        convert_records(AuditReader::new(bytes.as_slice()).unwrap(), Format::Json, &mut json_out).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&json_out).unwrap();
        assert_eq!(parsed["header"]["target_epoch"], 10);
        assert_eq!(parsed["footer"]["blocks_deleted"], 5);

        let mut jsonl_out = Vec::new();
        convert_records(
            AuditReader::new(bytes.as_slice()).unwrap(),
            Format::Jsonl,
            &mut jsonl_out,
        )
        .unwrap();
        let lines: Vec<&str> = std::str::from_utf8(&jsonl_out).unwrap().lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn rejects_audit_missing_header_or_footer() {
        let missing_header = encode_records(&[sample_footer()]);
        let err = convert_records(
            AuditReader::new(missing_header.as_slice()).unwrap(),
            Format::Jsonl,
            Vec::new(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("missing its header record"), "{err}");

        let missing_footer = encode_records(&[sample_header()]);
        let err = convert_records(
            AuditReader::new(missing_footer.as_slice()).unwrap(),
            Format::Jsonl,
            Vec::new(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("missing its footer record"), "{err}");
    }

    #[test]
    fn rejects_duplicate_header_or_footer() {
        let dup_header = encode_records(&[sample_header(), sample_header(), sample_footer()]);
        let err = convert_records(
            AuditReader::new(dup_header.as_slice()).unwrap(),
            Format::Jsonl,
            Vec::new(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("multiple header records"), "{err}");

        let dup_footer = encode_records(&[sample_header(), sample_footer(), sample_footer()]);
        let err = convert_records(
            AuditReader::new(dup_footer.as_slice()).unwrap(),
            Format::Jsonl,
            Vec::new(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("multiple footer records"), "{err}");
    }

    #[test]
    fn does_not_create_output_file_when_input_is_truncated() {
        let dir = std::env::temp_dir();
        let pid = std::process::id();
        let audit_path = dir.join(format!("tari_rollback_truncated_{pid}.audit"));
        let out_path = dir.join(format!("tari_rollback_truncated_{pid}.jsonl"));
        let _ = std::fs::remove_file(&out_path);

        let truncated = encode_records(&[sample_header()]);
        std::fs::write(&audit_path, &truncated).unwrap();

        for format in [Format::Jsonl, Format::Json] {
            let res = run(Args {
                audit: audit_path.clone(),
                format,
                out: Some(out_path.clone()),
            });
            assert!(res.is_err());
            assert!(!out_path.exists());
            assert!(!TempFileGuard::new(&out_path).path.exists());
        }
        let _ = std::fs::remove_file(&audit_path);
    }
}
