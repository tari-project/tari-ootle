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
use tari_ootle_walletd_client::{
    ComponentAddressOrName,
    WalletDaemonClient,
    types::{
        AccountInfo,
        AccountsCreateFreeTestCoinsRequest,
        AccountsCreateOrGetRequest,
        AccountsCreateRequest,
        AccountsGetBalancesRequest,
    },
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
    #[clap(long, short = 'a', alias = "account", conflicts_with = "account_name")]
    pub account: Option<ComponentAddressOrName>,
}

#[derive(Debug, Args, Clone)]
pub struct GetArgs {
    pub name: ComponentAddressOrName,
}

#[derive(Debug, Args, Clone)]
pub struct CreateFreeTestCoinsArgs {
    pub account_name: Option<ComponentAddressOrName>,
    #[clap(long, short = 'a', alias = "account", conflicts_with = "account_name")]
    pub account: Option<ComponentAddressOrName>,
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
    let account = args.account.or(args.account_name);
    let resp = client
        .get_account_balances(AccountsGetBalancesRequest { account, refresh: true })
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
    let account_arg = args.account.or(args.account_name);
    let account = match account_arg {
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
            // The claim creates the account and funds it, so it pays for a component, a vault, a
            // badge and the receipt. Defaulted well clear of that; the unspent remainder is refunded.
            max_fee: args.fee.unwrap_or(50_000),
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

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct CliTest {
        #[clap(subcommand)]
        command: AccountsSubcommand,
    }

    #[test]
    fn test_get_balances_args_parsing() {
        let cli_pos = CliTest::try_parse_from(["wallet", "get-balances", "my_positional_acc"])
            .expect("Failed to parse get-balances with positional argument");

        match cli_pos.command {
            AccountsSubcommand::GetBalances(args) => {
                assert!(matches!(
                    args.account_name,
                    Some(ComponentAddressOrName::Name(ref n)) if n == "my_positional_acc"
                ));
                assert_eq!(args.account, None);
            },
            _ => panic!("Expected AccountsSubcommand::GetBalances"),
        }

        let cli_named = CliTest::try_parse_from(["wallet", "get-balances", "--account", "my_special_acc"])
            .expect("Failed to parse get-balances with --account");

        match cli_named.command {
            AccountsSubcommand::GetBalances(args) => {
                assert_eq!(args.account_name, None);
                assert!(matches!(
                    args.account,
                    Some(ComponentAddressOrName::Name(ref n)) if n == "my_special_acc"
                ));
            },
            _ => panic!("Expected AccountsSubcommand::GetBalances"),
        }

        let cli_short = CliTest::try_parse_from(["wallet", "get-balances", "-a", "another_acc"])
            .expect("Failed to parse get-balances with -a");

        match cli_short.command {
            AccountsSubcommand::GetBalances(args) => {
                assert_eq!(args.account_name, None);
                assert!(matches!(
                    args.account,
                    Some(ComponentAddressOrName::Name(ref n)) if n == "another_acc"
                ));
            },
            _ => panic!("Expected AccountsSubcommand::GetBalances"),
        }

        let cli_default =
            CliTest::try_parse_from(["wallet", "get-balances"]).expect("Failed to parse get-balances without args");

        match cli_default.command {
            AccountsSubcommand::GetBalances(args) => {
                assert_eq!(args.account_name, None);
                assert_eq!(args.account, None);
            },
            _ => panic!("Expected AccountsSubcommand::GetBalances"),
        }
    }

    #[test]
    fn test_create_free_test_coins_args_parsing() {
        let cli_pos = CliTest::try_parse_from(["wallet", "create-free-test-coins", "my_positional_acc"])
            .expect("Failed to parse create-free-test-coins with positional argument");

        match cli_pos.command {
            AccountsSubcommand::CreateFreeTestCoins(args) => {
                assert!(matches!(
                    args.account_name,
                    Some(ComponentAddressOrName::Name(ref n)) if n == "my_positional_acc"
                ));
                assert_eq!(args.account, None);
            },
            _ => panic!("Expected AccountsSubcommand::CreateFreeTestCoins"),
        }

        let cli_flags = CliTest::try_parse_from([
            "wallet",
            "create-free-test-coins",
            "--account",
            "my_special_acc",
            "--fee",
            "1000",
            "--key",
            "2",
        ])
        .expect("Failed to parse create-free-test-coins with flags");

        match cli_flags.command {
            AccountsSubcommand::CreateFreeTestCoins(args) => {
                assert_eq!(args.account_name, None);
                assert!(matches!(
                    args.account,
                    Some(ComponentAddressOrName::Name(ref n)) if n == "my_special_acc"
                ));
                assert_eq!(args.fee, Some(1000));
                assert_eq!(args.key_id, Some(2));
            },
            _ => panic!("Expected AccountsSubcommand::CreateFreeTestCoins"),
        }

        let cli_short = CliTest::try_parse_from([
            "wallet",
            "create-free-test-coins",
            "-a",
            "another_acc",
            "-f",
            "500",
            "-k",
            "1",
        ])
        .expect("Failed to parse create-free-test-coins with short flags");

        match cli_short.command {
            AccountsSubcommand::CreateFreeTestCoins(args) => {
                assert_eq!(args.account_name, None);
                assert!(matches!(
                    args.account,
                    Some(ComponentAddressOrName::Name(ref n)) if n == "another_acc"
                ));
                assert_eq!(args.fee, Some(500));
                assert_eq!(args.key_id, Some(1));
            },
            _ => panic!("Expected AccountsSubcommand::CreateFreeTestCoins"),
        }
    }
}
