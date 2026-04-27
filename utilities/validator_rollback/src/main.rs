//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Offline break-glass rollback tool for Tari Ootle validator nodes.
//!
//! See `docs` / the rollback runbook for the operational workflow. The tool must only
//! be run against a stopped validator — RocksDB's LOCK file will block a concurrent
//! write open, which is exactly the safety behaviour we want.

use clap::{Parser, Subcommand};
use tari_validator_rollback::{apply, convert, inspect};

#[derive(Parser)]
#[clap(version, about = "Offline rollback tool for Tari Ootle validator state stores")]
struct Cli {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Roll the state store back to the `EpochCheckpoint` at `--target-epoch`.
    ///
    /// With `--dry-run`, the DB is opened read-only and only the audit file is produced
    /// — nothing on disk is mutated. Without it, the tool opens exclusive, applies the
    /// three primitives in a single write transaction, records a history breadcrumb,
    /// and emits the same audit file.
    Apply(apply::Args),

    /// Print a human-readable summary of an existing audit file.
    Inspect(inspect::Args),

    /// Re-serialise an audit file as JSON or JSONL for downstream tooling.
    Convert(convert::Args),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Apply(args) => apply::run(args),
        Command::Inspect(args) => inspect::run(args),
        Command::Convert(args) => convert::run(args),
    }
}
