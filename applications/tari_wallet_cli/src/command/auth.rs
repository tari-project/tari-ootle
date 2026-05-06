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
        AuthApiKeyCredentials,
        AuthCreateApiKeyRequest,
        AuthListApiKeysRequest,
        AuthListSessionsRequest,
        AuthLoginRequest,
        AuthRevokeApiKeyRequest,
        AuthRevokeTokenRequest,
        AuthCredentials,
    },
};

#[derive(Debug, Subcommand, Clone)]
pub enum AuthSubcommand {
    Request(RequestArgs),
    Revoke(RevokeArgs),
    List,
    #[command(subcommand)]
    ApiKey(ApiKeySubcommand),
}

#[derive(Debug, Subcommand, Clone)]
pub enum ApiKeySubcommand {
    Create(CreateApiKeyArgs),
    List,
    Revoke(RevokeApiKeyArgs),
    Authenticate(AuthenticateApiKeyArgs),
}

#[derive(Debug, Args, Clone)]
pub struct RequestArgs {
    permissions: Vec<JrpcPermission>,
}

#[derive(Debug, Args, Clone)]
pub struct RevokeArgs {
    permission_token_id: String,
}

#[derive(Debug, Args, Clone)]
pub struct CreateApiKeyArgs {
    #[clap(long)]
    name: String,
    #[clap(long)]
    allow_admin: bool,
    permissions: Vec<JrpcPermission>,
}

#[derive(Debug, Args, Clone)]
pub struct RevokeApiKeyArgs {
    id: String,
}

#[derive(Debug, Args, Clone)]
pub struct AuthenticateApiKeyArgs {
    #[clap(long)]
    api_key: String,
    permissions: Vec<JrpcPermission>,
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
            ApiKey(subcommand) => match subcommand {
                ApiKeySubcommand::Create(args) => {
                    let resp = client
                        .auth_create_api_key(AuthCreateApiKeyRequest {
                            name: args.name,
                            permissions: args.permissions,
                            allow_admin: args.allow_admin,
                        })
                        .await?;
                    println!("API key id: {}", resp.id);
                    println!("API key: {}", resp.api_key);
                },
                ApiKeySubcommand::List => {
                    let resp = client.auth_list_api_keys(AuthListApiKeysRequest {}).await?;
                    for key in &resp.api_keys {
                        println!(
                            "Id {} name {} permissions {}",
                            key.id,
                            key.name,
                            key.permissions.display()
                        );
                    }
                },
                ApiKeySubcommand::Revoke(args) => {
                    client.auth_revoke_api_key(AuthRevokeApiKeyRequest { id: args.id }).await?;
                    println!("API key revoked!");
                },
                ApiKeySubcommand::Authenticate(args) => {
                    let resp = client
                        .auth_request(AuthLoginRequest {
                            permissions: args.permissions,
                            credentials: AuthCredentials::ApiKey(AuthApiKeyCredentials { api_key: args.api_key }),
                        })
                        .await?;
                    println!("{}", resp.token);
                },
            },
        }
        Ok(())
    }
}
