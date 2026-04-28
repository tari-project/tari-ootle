//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{io::Write, path::PathBuf};

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

pub fn run(args: Args) -> anyhow::Result<()> {
    let reader = AuditReader::open(&args.audit).with_context(|| format!("opening {:?}", args.audit))?;

    let mut out: Box<dyn Write> = match args.out.as_ref() {
        Some(path) => Box::new(std::io::BufWriter::new(
            std::fs::File::create(path).with_context(|| format!("creating {:?}", path))?,
        )),
        None => Box::new(std::io::stdout().lock()),
    };

    match args.format {
        Format::Jsonl => {
            for record in reader.records() {
                let record = record?;
                serde_json::to_writer(&mut out, &record)?;
                writeln!(out)?;
            }
        },
        Format::Json => {
            let mut header = serde_json::Value::Null;
            let mut summaries = Vec::new();
            let mut transitions = Vec::new();
            let mut transactions_unfinalised = Vec::new();
            let mut footer = serde_json::Value::Null;
            for record in reader.records() {
                match record? {
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
    out.flush()?;
    Ok(())
}
