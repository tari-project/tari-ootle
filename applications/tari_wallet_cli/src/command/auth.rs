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
    WalletDaemonClient,
    permissions::JrpcPermission,
    types::{AuthCredentials, AuthListSessionsRequest, AuthLoginRequest, AuthRevokeTokenRequest},
};

use crate::{table::Table, table_row};

#[derive(Debug, Subcommand, Clone)]
pub enum AuthSubcommand {
    Request(RequestArgs),
    Revoke(RevokeArgs),
    List,
    #[clap(subcommand, alias = "api-key")]
    ApiKey(ApiKeySubcommand),
}

#[derive(Debug, Args, Clone)]
pub struct RequestArgs {
    permissions: Vec<JrpcPermission>,
}

#[derive(Debug, Args, Clone)]
pub struct RevokeArgs {
    permission_token_id: String,
}

#[derive(Debug, Subcommand, Clone)]
pub enum ApiKeySubcommand {
    Create(CreateApiKeyArgs),
    List,
    Revoke(RevokeApiKeyArgs),
}

#[derive(Debug, Args, Clone)]
pub struct CreateApiKeyArgs {
    name: String,
    permissions: Vec<JrpcPermission>,
    #[clap(long)]
    confirm_admin: bool,
}

#[derive(Debug, Args, Clone)]
pub struct RevokeApiKeyArgs {
    id: String,
}

impl AuthSubcommand {
    pub async fn handle(self, mut client: WalletDaemonClient) -> anyhow::Result<()> {
        #[allow(clippy::enum_glob_use)]
        use AuthSubcommand::*;
        match self {
            Request(args) => {
                if args.permissions.is_empty() {
                    println!("You forgot add permissions");
                } else {
                    let _resp = client
                        .auth_request(AuthLoginRequest {
                            permissions: args.permissions,
                            credentials: AuthCredentials::None,
                        })
                        .await?;
                    println!("Access granted");
                }
            },
            Revoke(args) => {
                client
                    .auth_revoke(AuthRevokeTokenRequest {
                        refresh_token_id: args.permission_token_id.parse()?,
                    })
                    .await?;
                println!("Token revoked!");
            },
            List => {
                let resp = client.auth_list_sessions(AuthListSessionsRequest {}).await?;
                for session in &resp.sessions {
                    println!("Id {} name {}", session.id, session.permissions.display());
                }
            },
            ApiKey(subcommand) => {
                handle_api_key_subcommand(subcommand, &mut client).await?;
            },
        }
        Ok(())
    }
}

async fn handle_api_key_subcommand(
    subcommand: ApiKeySubcommand,
    client: &mut WalletDaemonClient,
) -> anyhow::Result<()> {
    match subcommand {
        ApiKeySubcommand::Create(args) => handle_api_key_create(args, client).await?,
        ApiKeySubcommand::List => handle_api_key_list(client).await?,
        ApiKeySubcommand::Revoke(args) => handle_api_key_revoke(args, client).await?,
    }
    Ok(())
}

async fn handle_api_key_create(args: CreateApiKeyArgs, client: &mut WalletDaemonClient) -> anyhow::Result<()> {
    if args.permissions.is_empty() {
        anyhow::bail!("At least one permission is required");
    }

    let grant_admin = args.permissions.contains(&JrpcPermission::Admin);
    if grant_admin && !args.confirm_admin {
        anyhow::bail!("Admin scope requires --confirm-admin");
    }

    let resp = client
        .create_api_key(args.name, args.permissions, grant_admin)
        .await?;

    println!();
    println!("✅ API key created");
    println!("   id: {}", resp.id);
    println!("   name: {}", resp.name);
    println!("   permissions: {}", resp.permissions.display());
    println!("   created_at: {}", resp.created_at);
    if let Some(expires_at) = resp.expires_at {
        println!("   expires_at: {}", expires_at);
    }
    println!();
    println!("Plaintext key (shown once):");
    println!("{}", resp.key);
    println!();
    println!("Store this key securely. It will not be shown again.");
    Ok(())
}

async fn handle_api_key_list(client: &mut WalletDaemonClient) -> anyhow::Result<()> {
    let resp = client.list_api_keys().await?;

    if resp.keys.is_empty() {
        println!("No API keys found");
        return Ok(());
    }

    let mut table = Table::new();
    table.enable_row_count();
    table.set_titles(vec!["Name", "Id", "Permissions", "Created", "Last used", "Expires"]);
    for key in resp.keys {
        table.add_row(table_row![
            key.name,
            key.id,
            key.permissions.display(),
            key.created_at,
            key.last_used
                .map(|value| value.to_string())
                .unwrap_or_else(|| "Never".to_string()),
            key.expires_at
                .map(|value| value.to_string())
                .unwrap_or_else(|| "Never".to_string()),
        ]);
    }
    table.print_stdout();
    Ok(())
}

async fn handle_api_key_revoke(args: RevokeApiKeyArgs, client: &mut WalletDaemonClient) -> anyhow::Result<()> {
    client.revoke_api_key(args.id).await?;
    println!("API key revoked");
    Ok(())
}
