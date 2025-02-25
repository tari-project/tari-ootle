//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use clap::{Args, Subcommand};
use tari_dan_common_types::{shard::Shard, ShardGroup};
use tari_template_lib::models::Amount;
use tari_wallet_daemon_client::{
    types::{AccountOrKeyIndex, ClaimValidatorFeesRequest, GetValidatorFeesRequest},
    ComponentAddressOrName,
    WalletDaemonClient,
};

use crate::command::transaction::summarize_finalize_result;

#[derive(Debug, Subcommand, Clone)]
pub enum ValidatorSubcommand {
    ClaimFees(ClaimFeesArgs),
    GetFees(GetFeesArgs),
}

#[derive(Debug, Args, Clone)]
pub struct ClaimFeesArgs {
    #[clap(long, short = 'a', alias = "account")]
    pub dest_account_name: Option<String>,
    #[clap(long, short = 's', value_parser = parse_shard)]
    pub shard: Shard,
    #[clap(long)]
    pub max_fee: Option<u32>,
    #[clap(long)]
    pub dry_run: bool,
}

fn parse_shard(s: &str) -> Result<Shard, String> {
    Ok(Shard::from(
        s.parse::<u32>()
            .map_err(|_| "Invalid shard. Expected a number.".to_string())?,
    ))
}

#[derive(Debug, Args, Clone)]
pub struct GetFeesArgs {
    #[clap(long, short = 'a')]
    pub account: Option<ComponentAddressOrName>,
    #[clap(long, short = 'g')]
    pub shard_group: Option<ShardGroup>,
}

impl ValidatorSubcommand {
    pub async fn handle(self, mut client: WalletDaemonClient) -> Result<(), anyhow::Error> {
        match self {
            ValidatorSubcommand::ClaimFees(args) => {
                handle_claim_validator_fees(args, &mut client).await?;
            },
            ValidatorSubcommand::GetFees(args) => {
                handle_get_fees(args, &mut client).await?;
            },
        }
        Ok(())
    }
}

pub async fn handle_get_fees(args: GetFeesArgs, client: &mut WalletDaemonClient) -> Result<(), anyhow::Error> {
    let resp = client
        .get_validator_fees(GetValidatorFeesRequest {
            account_or_key: AccountOrKeyIndex::Account(args.account),
            shard_group: args.shard_group,
        })
        .await?;

    println!("Validator fees:");
    for (shard, fee) in resp.fees {
        println!("{}: {}XTR at address {}", shard, fee.amount, fee.address);
    }
    Ok(())
}

pub async fn handle_claim_validator_fees(
    args: ClaimFeesArgs,
    client: &mut WalletDaemonClient,
) -> Result<(), anyhow::Error> {
    let ClaimFeesArgs {
        dest_account_name,
        shard,
        max_fee,
        dry_run,
    } = args;

    println!("Submitting claim validator fees transaction...");

    let resp = client
        .claim_validator_fees(ClaimValidatorFeesRequest {
            account: dest_account_name
                .map(|name| ComponentAddressOrName::from_str(&name))
                .transpose()?,
            claim_key_index: None,
            max_fee: max_fee.map(Amount::from),
            shards: vec![shard],
            dry_run,
        })
        .await?;

    println!("Transaction: {}", resp.transaction_id);
    println!("Fee: {}", resp.fee);
    println!();
    summarize_finalize_result(&resp.result);

    Ok(())
}
