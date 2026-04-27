//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::path::PathBuf;

use anyhow::Context;
use clap::Args as ClapArgs;

use crate::audit::{AuditReader, AuditRecord};

#[derive(Debug, ClapArgs)]
pub struct Args {
    #[clap(long)]
    audit: PathBuf,
}

pub fn run(args: Args) -> anyhow::Result<()> {
    let reader = AuditReader::open(&args.audit).with_context(|| format!("opening {:?}", args.audit))?;

    let mut header_printed = false;
    let mut footer_printed = false;

    println!("Rollback audit: {}", args.audit.display());
    for record in reader.records() {
        let record = record?;
        match record {
            AuditRecord::Header(h) => {
                println!("  Format version: 1");
                println!(
                    "  Generated:      unix={}  {}",
                    h.generated_at_unix_secs,
                    if h.dry_run { "(dry-run)" } else { "(applied)" }
                );
                println!("  Target epoch:   {}", h.target_epoch);
                println!(
                    "  Tip before:     {}",
                    match (h.pre_rollback_tip_epoch, h.pre_rollback_tip_block) {
                        (Some(e), Some(b)) => format!("epoch {} / {}", e, b),
                        _ => "unknown".to_string(),
                    }
                );
                println!(
                    "  Shard group:    {}..={}",
                    h.shard_group.start, h.shard_group.end_inclusive
                );
                let versions: Vec<String> = h
                    .state_version_per_shard
                    .iter()
                    .map(|(s, v)| match s {
                        crate::audit::AuditShard::Global => format!("global → {v}"),
                        crate::audit::AuditShard::Numbered(n) => format!("shard {n} → {v}"),
                    })
                    .collect();
                println!("  State versions: {}", versions.join(", "));
                println!("  Tool version:   {}", h.tool_version);
                header_printed = true;
            },
            AuditRecord::Footer(f) => {
                println!("Impact:");
                println!("  Substates removed:        {}", f.substates_removed);
                println!("  Substates rewound:        {}", f.substates_rewound);
                println!("  Substate transitions:     {}", f.substate_transitions);
                println!("  Transactions unfinalised: {}", f.transactions_unfinalised);
                println!("  Blocks deleted:           {}", f.blocks_deleted);
                footer_printed = true;
            },
            _ => {}, // body records skipped by inspect — use `convert` for detail
        }
    }

    if !header_printed {
        anyhow::bail!("audit file is missing its header record");
    }
    if !footer_printed {
        anyhow::bail!("audit file is missing its footer record (truncated?)");
    }
    Ok(())
}
