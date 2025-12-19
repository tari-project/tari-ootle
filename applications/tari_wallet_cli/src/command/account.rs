//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use clap::{Args, Subcommand};
use tari_ootle_common_types::displayable::Displayable;
use tari_wallet_daemon_client::{
    types::{
        AccountInfo,
        AccountsCreateFreeTestCoinsRequest,
        AccountsCreateOrGetRequest,
        AccountsCreateRequest,
        AccountsGetBalancesRequest,
    },
    ComponentAddressOrName,
    WalletDaemonClient,
};

use crate::{table::Table, table_row};

#[derive(Debug, Subcommand, Clone)]
pub enum AccountsSubcommand {
    #[clap(alias = "new")]
    Create(CreateArgs),
    #[clap(alias = "get-balance", alias = "balance")]
    GetBalances(GetBalancesArgs),
    List,

    Get(GetArgs),
    #[clap(alias = "faucet")]
    CreateFreeTestCoins(CreateFreeTestCoinsArgs),
    #[clap(alias = "default")]
    SetDefault(SetDefaultArgs),
}

#[derive(Debug, Args, Clone)]
pub struct CreateArgs {
    #[clap(long, alias = "name")]
    pub account_name: Option<String>,
    pub is_default: bool,
    #[clap(long, short, alias = "key")]
    pub key_id: Option<u64>,
}

#[derive(Debug, Args, Clone)]
pub struct SetDefaultArgs {
    pub account_name: ComponentAddressOrName,
}

#[derive(Debug, Args, Clone)]
pub struct GetBalancesArgs {
    pub account_name: Option<ComponentAddressOrName>,
}

#[derive(Debug, Args, Clone)]
pub struct GetArgs {
    pub name: ComponentAddressOrName,
}

#[derive(Debug, Args, Clone)]
pub struct CreateFreeTestCoinsArgs {
    pub account: Option<ComponentAddressOrName>,
    #[clap(long, short, alias = "amount")]
    pub amount: Option<u64>,
    #[clap(long, short, alias = "fee")]
    pub fee: Option<u64>,
    #[clap(long, short, alias = "key")]
    pub key_id: Option<u64>,
}

impl AccountsSubcommand {
    pub async fn handle(self, mut client: WalletDaemonClient) -> Result<(), anyhow::Error> {
        match self {
            AccountsSubcommand::Create(args) => {
                handle_create(args, &mut client).await?;
            },
            AccountsSubcommand::GetBalances(args) => {
                handle_get_balances(args, &mut client).await?;
            },
            AccountsSubcommand::List => {
                handle_list(&mut client).await?;
            },
            AccountsSubcommand::Get(args) => handle_get(args, &mut client).await?,
            AccountsSubcommand::CreateFreeTestCoins(args) => handle_create_free_test_coins(args, &mut client).await?,
            AccountsSubcommand::SetDefault(args) => handle_set_default(args, &mut client).await?,
        }
        Ok(())
    }
}

async fn handle_create(args: CreateArgs, client: &mut WalletDaemonClient) -> Result<(), anyhow::Error> {
    let resp = client
        .create_account(AccountsCreateRequest {
            account_name: args.account_name,
            is_default: Some(args.is_default),
            key_index: args.key_id,
        })
        .await?;

    println!();
    println!("✅ Account created (Locally, not on-chain)");
    println!("   component address: {}", resp.account.component_address);
    println!("   address: {}", resp.address);
    Ok(())
}

async fn handle_set_default(args: SetDefaultArgs, client: &mut WalletDaemonClient) -> Result<(), anyhow::Error> {
    let _resp = client.accounts_set_default(args.account_name).await?;
    println!("✅ Default account set");
    Ok(())
}

async fn handle_get_balances(args: GetBalancesArgs, client: &mut WalletDaemonClient) -> Result<(), anyhow::Error> {
    let resp = client
        .get_account_balances(AccountsGetBalancesRequest {
            account: args.account_name,
            refresh: true,
        })
        .await?;

    if resp.balances.is_empty() {
        println!("Account {} has no vaults", resp.address);
        return Ok(());
    }

    println!("Account {} balances:", resp.address);
    println!();
    let mut table = Table::new();
    table.enable_row_count();
    table.set_titles(vec!["VaultId", "Resource", "Balance"]);
    for balance in resp.balances {
        table.add_row(table_row!(
            balance.vault_address.display(),
            format!("{} {:?}", balance.resource_address, balance.resource_type),
            balance.to_balance_string()
        ));
    }
    table.print_stdout();
    Ok(())
}

async fn handle_create_free_test_coins(
    args: CreateFreeTestCoinsArgs,
    client: &mut WalletDaemonClient,
) -> Result<(), anyhow::Error> {
    println!("Creating free test coins...");
    let account = match args.account {
        Some(account) => {
            let resp = client
                .create_or_get_account(AccountsCreateOrGetRequest {
                    account: Some(account),
                    is_default: None,
                    key_index: args.key_id,
                })
                .await?;
            resp.account
        },
        None => {
            // Create a new account
            let resp = client
                .create_account(AccountsCreateRequest {
                    account_name: None,
                    is_default: None,
                    key_index: args.key_id,
                })
                .await?;
            resp.account
        },
    };

    let resp = client
        .create_free_test_coins(AccountsCreateFreeTestCoinsRequest {
            account: account.component_address.into(),
            // Default 1 tXTR
            amount: args.amount.unwrap_or(1_000_000).into(),
            max_fee: args.fee,
        })
        .await?;

    println!("✅ Free test coins created");
    println!("   amount: {}", resp.amount);
    println!("   transaction fee: {}", resp.fee);
    Ok(())
}

async fn handle_list(client: &mut WalletDaemonClient) -> Result<(), anyhow::Error> {
    let resp = client.list_accounts(0, 100).await?;

    if resp.accounts.is_empty() {
        println!("No accounts found");
        return Ok(());
    }

    let mut table = Table::new();
    table.enable_row_count();
    table.set_titles(vec!["Name", "Component", "Address", "Default"]);
    println!("Accounts:");
    for AccountInfo { account, address } in resp.accounts {
        table.add_row(table_row!(
            account.name.as_deref().unwrap_or("<None>"),
            account.component_address,
            address,
            if account.is_default { "✅" } else { "" }
        ));
    }
    table.print_stdout();
    Ok(())
}

async fn handle_get(args: GetArgs, client: &mut WalletDaemonClient) -> Result<(), anyhow::Error> {
    println!("Get account component address by its name...");
    let resp = client.accounts_get(args.name.clone()).await?;

    println!(
        "Account {} substate_address: {}",
        resp.account.name.as_deref().unwrap_or("<None>"),
        resp.account.component_address
    );
    println!();

    Ok(())
}
