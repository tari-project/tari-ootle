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
        AuthCreateApiKeyRequest,
        AuthCredentials,
        AuthListApiKeysRequest,
        AuthListSessionsRequest,
        AuthLoginRequest,
        AuthRevokeApiKeyRequest,
        AuthRevokeTokenRequest,
    },
};

#[derive(Debug, Subcommand, Clone)]
pub enum AuthSubcommand {
    Request(RequestArgs),
    Revoke(RevokeArgs),
    List,
    /// Manage long-lived API keys for AI agents and automated clients.
    /// All subcommands require the active session to hold the `Admin` permission.
    #[clap(subcommand)]
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
    /// Mint a new long-lived API key. The raw key is printed exactly once
    /// — store it immediately. The daemon persists only a SHA-256 hash.
    Create(CreateApiKeyArgs),
    /// List all API keys (active and revoked) with granted scopes and
    /// last-used timestamps.
    List,
    /// Revoke an API key by id. Revocation is effective immediately.
    Revoke(RevokeApiKeyArgs),
}

#[derive(Debug, Args, Clone)]
pub struct CreateApiKeyArgs {
    /// Human-readable label for this key. Shown in `list`.
    #[clap(long)]
    pub name: String,
    /// Permission scopes, comma-separated. Example:
    /// `--permissions AccountInfo,TransactionGet`.
    #[clap(long, value_delimiter = ',')]
    pub permissions: Vec<JrpcPermission>,
    /// Required if `permissions` includes `Admin`.
    #[clap(long)]
    pub confirm_admin: bool,
}

#[derive(Debug, Args, Clone)]
pub struct RevokeApiKeyArgs {
    /// Numeric id of the key to revoke. See `auth api-key list`.
    #[clap(long)]
    pub id: i64,
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
            ApiKey(sub) => sub.handle(client).await?,
        }
        Ok(())
    }
}

impl ApiKeySubcommand {
    pub async fn handle(self, mut client: WalletDaemonClient) -> anyhow::Result<()> {
        #[allow(clippy::enum_glob_use)]
        use ApiKeySubcommand::*;
        match self {
            Create(args) => {
                if args.permissions.is_empty() {
                    anyhow::bail!("--permissions must contain at least one scope; refusing to mint an unusable key");
                }
                let perm_strings: Vec<String> = args.permissions.iter().map(|p| p.to_string()).collect();
                let resp = client
                    .auth_create_api_key(AuthCreateApiKeyRequest {
                        name: args.name,
                        permissions: perm_strings,
                        confirm_admin: args.confirm_admin,
                    })
                    .await?;
                println!("API key created.");
                println!("  id:   {}", resp.id);
                println!("  name: {}", resp.name);
                println!();
                println!("KEY (shown ONCE — store immediately):");
                println!("{}", resp.key);
            },
            List => {
                let resp = client.auth_list_api_keys(AuthListApiKeysRequest {}).await?;
                if resp.keys.is_empty() {
                    println!("No API keys issued.");
                    return Ok(());
                }
                for k in &resp.keys {
                    let status = if k.revoked_at.is_some() { "REVOKED" } else { "active" };
                    let last_used = k
                        .last_used_at
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "never".to_string());
                    println!(
                        "[{}] id={} name={:?} created={} last_used={}",
                        status, k.id, k.name, k.created_at, last_used
                    );
                }
            },
            Revoke(args) => {
                client
                    .auth_revoke_api_key(AuthRevokeApiKeyRequest { id: args.id })
                    .await?;
                println!("API key {} revoked.", args.id);
            },
        }
        Ok(())
    }
}
