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
    WalletDaemonClient,
    permissions::JrpcPermission,
    types::{AuthCredentials, AuthListSessionsRequest, AuthLoginRequest, AuthRevokeTokenRequest},
};

#[derive(Debug, Subcommand, Clone)]
pub enum AuthSubcommand {
    Request(RequestArgs),
    Revoke(RevokeArgs),
    List,
}

#[derive(Debug, Args, Clone)]
pub struct RequestArgs {
    permissions: Vec<JrpcPermission>,
}

#[derive(Debug, Args, Clone)]
pub struct RevokeArgs {
    permission_token_id: String,
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
        }
        Ok(())
    }
}
