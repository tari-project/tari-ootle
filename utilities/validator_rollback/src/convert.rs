//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    io::{Read, Write},
    path::PathBuf,
};

use anyhow::Context;
use clap::{ArgEnum, Args as ClapArgs};

use crate::audit::{AuditReader, AuditRecord};

#[derive(Debug, ClapArgs)]
pub struct Args {
    #[clap(long)]
    pub audit: PathBuf,

    #[clap(long, arg_enum)]
    pub format: Format,

    /// Output path. If omitted, writes to stdout.
    #[clap(long)]
    pub out: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ArgEnum)]
pub enum Format {
    /// One JSON object per line, `kind` field discriminating variants.
    Jsonl,
    /// Single JSON document — `header`, `summaries`, `transitions`, `transactions_unfinalised`, `footer`.
    Json,
}

pub fn run(args: Args) -> anyhow::Result<()> {
    let reader = AuditReader::open(&args.audit).with_context(|| format!("opening {:?}", args.audit))?;
    let records = read_validated_records(reader)?;

    let mut out: Box<dyn Write> = match args.out.as_ref() {
        Some(path) => Box::new(std::io::BufWriter::new(
            std::fs::File::create(path).with_context(|| format!("creating {:?}", path))?,
        )),
        None => Box::new(std::io::stdout().lock()),
    };

    write_records(&records, args.format, &mut out)?;
    out.flush()?;
    Ok(())
}

fn read_validated_records<R: Read>(reader: AuditReader<R>) -> anyhow::Result<Vec<AuditRecord>> {
    let mut records = Vec::new();
    let mut header_seen = false;
    let mut footer_seen = false;

    for record in reader.records() {
        let record = record?;
        match &record {
            AuditRecord::Header(_) => {
                if header_seen {
                    anyhow::bail!("audit file contains multiple header records");
                }
                header_seen = true;
            },
            AuditRecord::Footer(_) => {
                if footer_seen {
                    anyhow::bail!("audit file contains multiple footer records");
                }
                footer_seen = true;
            },
            _ => {},
        }
        records.push(record);
    }

    if !header_seen {
        anyhow::bail!("audit file is missing its header record");
    }
    if !footer_seen {
        anyhow::bail!("audit file is missing its footer record (truncated?)");
    }
    Ok(records)
}

fn write_records<W: Write>(records: &[AuditRecord], format: Format, mut out: W) -> anyhow::Result<()> {
    match format {
        Format::Jsonl => {
            for record in records {
                serde_json::to_writer(&mut out, record)?;
                writeln!(out)?;
            }
        },
        Format::Json => {
            let mut header = serde_json::Value::Null;
            let mut summaries = Vec::new();
            let mut transitions = Vec::new();
            let mut transactions_unfinalised = Vec::new();
            let mut footer = serde_json::Value::Null;
            for record in records {
                match record {
                    AuditRecord::Header(h) => header = serde_json::to_value(h)?,
                    AuditRecord::SubstateSummary(s) => summaries.push(serde_json::to_value(s)?),
                    AuditRecord::SubstateTransition(t) => transitions.push(serde_json::to_value(t)?),
                    AuditRecord::TransactionUnfinalised(t) => transactions_unfinalised.push(serde_json::to_value(t)?),
                    AuditRecord::Footer(f) => footer = serde_json::to_value(f)?,
                }
            }
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

        let reader = AuditReader::new(bytes.as_slice()).unwrap();
        let records = read_validated_records(reader).unwrap();

        let mut json_out = Vec::new();
        write_records(&records, Format::Json, &mut json_out).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&json_out).unwrap();
        assert_eq!(parsed["header"]["target_epoch"], 10);
        assert_eq!(parsed["footer"]["blocks_deleted"], 5);

        let mut jsonl_out = Vec::new();
        write_records(&records, Format::Jsonl, &mut jsonl_out).unwrap();
        let lines: Vec<&str> = std::str::from_utf8(&jsonl_out).unwrap().lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn rejects_audit_missing_header_or_footer() {
        let missing_header = encode_records(&[sample_footer()]);
        let err = read_validated_records(AuditReader::new(missing_header.as_slice()).unwrap()).unwrap_err();
        assert!(err.to_string().contains("missing its header record"), "{err}");

        let missing_footer = encode_records(&[sample_header()]);
        let err = read_validated_records(AuditReader::new(missing_footer.as_slice()).unwrap()).unwrap_err();
        assert!(err.to_string().contains("missing its footer record"), "{err}");
    }

    #[test]
    fn rejects_duplicate_header_or_footer() {
        let dup_header = encode_records(&[sample_header(), sample_header(), sample_footer()]);
        let err = read_validated_records(AuditReader::new(dup_header.as_slice()).unwrap()).unwrap_err();
        assert!(err.to_string().contains("multiple header records"), "{err}");

        let dup_footer = encode_records(&[sample_header(), sample_footer(), sample_footer()]);
        let err = read_validated_records(AuditReader::new(dup_footer.as_slice()).unwrap()).unwrap_err();
        assert!(err.to_string().contains("multiple footer records"), "{err}");
    }

    #[test]
    fn does_not_create_output_file_when_input_is_truncated() {
        let dir = std::env::temp_dir();
        let pid = std::process::id();
        let audit_path = dir.join(format!("tari_rollback_truncated_{pid}.audit"));
        let out_path = dir.join(format!("tari_rollback_truncated_{pid}.json"));
        let _ = std::fs::remove_file(&out_path);

        let truncated = encode_records(&[sample_header()]);
        std::fs::write(&audit_path, &truncated).unwrap();

        let res = run(Args {
            audit: audit_path.clone(),
            format: Format::Json,
            out: Some(out_path.clone()),
        });
        let _ = std::fs::remove_file(&audit_path);

        assert!(res.is_err());
        assert!(!out_path.exists());
    }
}
