//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod cli;
mod transaction_writer;

use std::{
    collections::HashMap,
    fs,
    io,
    io::{BufRead, Seek, SeekFrom, Write, stdout},
};

use anyhow::anyhow;
use cli::Cli;
use tari_crypto::{keys::SecretKey, ristretto::RistrettoSecretKey, tari_utilities::hex::Hex};
use tari_ootle_common_types::SubstateRequirement;
use tari_ootle_transaction::{Blob, Network};
use tari_template_lib_types::TemplateAddress;
use tari_transaction_manifest::ManifestValue;
use transaction_generator::{
    BoxedTransactionBuilder,
    read_number_of_transactions,
    read_transactions,
    transaction_builders::{free_coins, manifest},
};

use crate::{
    cli::{SubCommand, WriteArgs},
    transaction_writer::write_transactions,
};

fn main() -> anyhow::Result<()> {
    let cli = Cli::init();
    match cli.sub_command {
        SubCommand::Write(args) => {
            if !args.overwrite && args.output_file.exists() {
                anyhow::bail!("Output file {} already exists", args.output_file.display());
            }

            let timer = std::time::Instant::now();
            println!("Generating and writing {} transactions", args.num_transactions,);

            let mut file = std::fs::File::create(&args.output_file)?;

            let builder = get_transaction_builder(&args)?;
            write_transactions(
                args.num_transactions,
                builder,
                &|_| {
                    print!(".");
                    stdout().flush().unwrap()
                },
                &mut file,
            )?;
            println!();
            let size = file.metadata()?.len() / 1024 / 1024;
            println!(
                "Wrote {} transactions to {} ({} MiB) in {:.2?}",
                args.num_transactions,
                args.output_file.display(),
                size,
                timer.elapsed()
            );
        },
        SubCommand::Read(args) => {
            let mut file = fs::File::open(args.input_file)?;

            let num_transactions = read_number_of_transactions(&mut file)?;
            println!("Number of transactions: {}", num_transactions);
            file.seek(SeekFrom::Start(0))?;
            let receiver = read_transactions(file, 0)?;

            while let Ok(transaction) = receiver.recv() {
                println!("Read transaction: {}", transaction.calculate_id());
            }
        },
    }

    Ok(())
}

fn get_transaction_builder(args: &WriteArgs) -> anyhow::Result<BoxedTransactionBuilder> {
    let network = args.network.unwrap_or(Network::LocalNet);
    match args.manifest.as_ref() {
        Some(manifest) => {
            if args.random_signer && args.signer_secret_key.is_none() {
                anyhow::bail!(
                    "--random-signer requires --signer: a fresh random key seals each transaction, while the --signer \
                     key is added as an additional signer (e.g. it owns the fee-paying account and authorises pay_fee)"
                );
            }
            let signer_key = args
                .signer_secret_key
                .as_ref()
                .map(|s| RistrettoSecretKey::from_hex(s))
                .transpose()
                .map_err(|_| anyhow!("Failed to parse secret"))?
                .unwrap_or_else(|| RistrettoSecretKey::random(&mut rand::rng()));
            let mut manifest_args = parse_args(&args.manifest_args)?;
            if let Some(args_file) = &args.manifest_args_file {
                let file = io::BufReader::new(fs::File::open(args_file)?);
                for ln in file.lines() {
                    let ln = ln?;
                    let line = ln.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    manifest_args.extend(parse_arg(line));
                }
            }
            let templates = parse_templates(&args.templates)?;
            let inputs = parse_inputs(&args.inputs)?;
            let blobs = parse_blobs(&args.blobs)?;
            manifest::builder(
                signer_key,
                network,
                manifest,
                manifest_args,
                templates,
                inputs,
                blobs,
                args.random_signer,
            )
        },
        None => Ok(Box::new(free_coins::builder(network))),
    }
}

fn parse_inputs(items: &[String]) -> anyhow::Result<Vec<SubstateRequirement>> {
    items
        .iter()
        .map(|s| {
            s.trim()
                .parse::<SubstateRequirement>()
                .map_err(|e| anyhow!("Invalid --input '{}': {}", s, e))
        })
        .collect()
}

fn parse_blobs(items: &[String]) -> anyhow::Result<HashMap<String, Blob>> {
    items
        .iter()
        .map(|s| {
            let (name, path) = s
                .split_once('=')
                .ok_or_else(|| anyhow!("Invalid --blob mapping '{}' (expected <name>=<file_path>)", s))?;
            let name = name.trim();
            let path = path.trim();
            let bytes =
                fs::read(path).map_err(|e| anyhow!("Failed to read blob '{}' from file '{}': {}", name, path, e))?;
            Ok((name.to_string(), Blob::from(bytes)))
        })
        .collect()
}

fn parse_templates(items: &[String]) -> anyhow::Result<HashMap<String, TemplateAddress>> {
    items
        .iter()
        .map(|s| {
            let (alias, address) = s
                .split_once('=')
                .ok_or_else(|| anyhow!("Invalid template mapping '{}' (expected <alias>=template_<hex>)", s))?;
            let trimmed = address.trim();
            let hex = trimmed.strip_prefix("template_").unwrap_or(trimmed).trim();
            let address = TemplateAddress::from_hex(hex)
                .map_err(|_| anyhow!("Invalid template address for '{}': {}", alias, trimmed))?;
            Ok((alias.trim().to_string(), address))
        })
        .collect()
}

fn parse_args(globals: &[String]) -> Result<HashMap<String, ManifestValue>, anyhow::Error> {
    globals.iter().map(|s| parse_arg(s)).collect()
}

fn parse_arg(arg: &str) -> Result<(String, ManifestValue), anyhow::Error> {
    let (name, value) = arg.split_once('=').ok_or_else(|| anyhow!("Invalid arg: {}", arg))?;
    let value = value
        .trim()
        .parse()
        .map_err(|err| anyhow!("Failed to parse arg '{}': {}", name, err))?;
    Ok((name.trim().to_string(), value))
}
