//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use tari_ootle_transaction::Network;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
pub struct Cli {
    #[clap(subcommand)]
    pub sub_command: SubCommand,
}

impl Cli {
    pub fn init() -> Self {
        Self::parse()
    }
}

#[derive(Subcommand, Debug)]
pub enum SubCommand {
    Write(WriteArgs),
    Read(ReadArgs),
}

#[derive(Args, Debug)]
pub struct WriteArgs {
    #[clap(long, short = 'n')]
    pub num_transactions: u64,
    #[clap(long, short = 'o')]
    pub output_file: PathBuf,
    #[clap(long)]
    pub overwrite: bool,
    #[clap(long, short = 'm')]
    pub manifest: Option<PathBuf>,
    #[clap(long, short = 'a', alias = "arg")]
    pub manifest_args: Vec<String>,
    /// Template alias to address mappings used to resolve `use <alias>;` imports in the manifest,
    /// e.g. `--template MaxCompute=template_<hex>`. May be repeated.
    #[clap(long = "template", alias = "templates")]
    pub templates: Vec<String>,
    /// Extra substate inputs to declare on each generated transaction that aren't passed as `--arg`
    /// (e.g. a fee vault the manifest debits but never names). May be repeated. Accepts an
    /// unversioned id like `vault_<hex>` or a versioned `vault_<hex>:<version>`. Substate-typed
    /// `--arg`s are declared as inputs automatically and don't need repeating here.
    #[clap(long = "input", alias = "inputs")]
    pub inputs: Vec<String>,
    #[clap(long, alias = "args-file")]
    pub manifest_args_file: Option<PathBuf>,
    /// Load a file as a named manifest blob, e.g. `--blob template=path/to/template.wasm`. The name
    /// is resolved by `blob!(name)` / `publish_template!(name)` in the manifest and the file's bytes
    /// become the blob payload. May be repeated.
    #[clap(long = "blob", alias = "blobs")]
    pub blobs: Vec<String>,
    /// Seal each generated transaction with a fresh random keypair instead of with `--signer`, and
    /// add `--signer` as an additional signer (so its badge still authorises e.g. fee payment from
    /// its account). Requires `--signer`. Primarily useful for publishing the same template binary
    /// repeatedly: the random seal key becomes the template's author, so each publish yields a unique
    /// template address (H(author_public_key, binary_hash)) instead of colliding on a duplicate
    /// substate.
    #[clap(long, alias = "random-author")]
    pub random_signer: bool,
    #[clap(long, short = 'k', alias = "signer")]
    pub signer_secret_key: Option<String>,
    #[clap(long, short = 't')]
    pub network: Option<Network>,
}
#[derive(Args, Debug)]
pub struct ReadArgs {
    #[clap(long, short = 'f')]
    pub input_file: PathBuf,
}
