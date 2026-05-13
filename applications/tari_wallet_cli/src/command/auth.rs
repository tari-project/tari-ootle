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
    types::{
        AuthCreateApiKeyRequest, AuthCredentials, AuthListApiKeysRequest, AuthListSessionsRequest, AuthLoginRequest,
        AuthRevokeApiKeyRequest, AuthRevokeTokenRequest,
    },
};

#[derive(Debug, Subcommand, Clone)]
pub enum AuthSubcommand {
    Request(RequestArgs),
    Revoke(RevokeArgs),
    List,
    #[clap(subcommand, alias = "api-keys")]
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
    /// Create a named API key and print the raw key once.
    Create(ApiKeyCreateArgs),
    /// List API keys and their current revocation state.
    List,
    /// Revoke an API key by id.
    Revoke(ApiKeyRevokeArgs),
}

#[derive(Debug, Args, Clone)]
pub struct ApiKeyCreateArgs {
    /// Human-readable key name shown in the wallet UI and audit output.
    name: String,
    /// Permission grants for the key. Admin requires --confirm-admin.
    permissions: Vec<JrpcPermission>,
    /// Explicitly confirm that an API key should receive Admin.
    #[arg(long)]
    confirm_admin: bool,
}

#[derive(Debug, Args, Clone)]
pub struct ApiKeyRevokeArgs {
    api_key_id: i32,
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
            ApiKey(subcommand) => handle_api_key(subcommand, client).await?,
        }
        Ok(())
    }
}

async fn handle_api_key(subcommand: ApiKeySubcommand, mut client: WalletDaemonClient) -> anyhow::Result<()> {
    match subcommand {
        ApiKeySubcommand::Create(args) => {
            let resp = client
                .auth_create_api_key(AuthCreateApiKeyRequest {
                    name: args.name,
                    permissions: args.permissions,
                    confirm_admin: args.confirm_admin,
                })
                .await?;
            println!("API key created with id {}", resp.key.id);
            println!("Name: {}", resp.key.name);
            println!("Permissions: {}", resp.key.permissions.display());
            println!("Raw key: {}", resp.api_key);
            println!("Store this raw key now. It cannot be retrieved again.");
        },
        ApiKeySubcommand::List => {
            let resp = client.auth_list_api_keys(AuthListApiKeysRequest {}).await?;
            for key in resp.api_keys {
                let state = if let Some(revoked_at) = key.revoked_at {
                    format!("revoked at {revoked_at}")
                } else {
                    "active".to_string()
                };
                let last_used = key
                    .last_used_at
                    .map(|last_used_at| last_used_at.to_string())
                    .unwrap_or_else(|| "never".to_string());
                let expires = key
                    .expires_at
                    .map(|expires_at| expires_at.to_string())
                    .unwrap_or_else(|| "no-expiry".to_string());
                println!(
                    "Id {} name {} state {} last-used {} expires {} permissions {}",
                    key.id,
                    key.name,
                    state,
                    last_used,
                    expires,
                    key.permissions.display()
                );
            }
        },
        ApiKeySubcommand::Revoke(args) => {
            client
                .auth_revoke_api_key(AuthRevokeApiKeyRequest { id: args.api_key_id })
                .await?;
            println!("API key revoked!");
        },
    }
    Ok(())
}
