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

use anyhow::anyhow;
use clap::{Args, Subcommand};
use tari_ootle_walletd_client::{WalletDaemonClient, types::WebRtcStartRequest};
use url::Url;

#[derive(Debug, Subcommand, Clone)]
pub enum WebRtcSubcommand {
    #[clap(alias = "start")]
    Start(StartArgs),
}

#[derive(Debug, Args, Clone)]
pub struct StartArgs {
    #[clap(long, alias = "url")]
    pub signaling_server_url: Option<Url>,
    #[clap(long, alias = "token")]
    pub signaling_server_token: Option<String>,
    #[clap(long, alias = "permissions")]
    pub webrtc_permissions_token: Option<String>,
}

fn resolve_start_params(
    url: Option<&Url>,
    cli_token: Option<String>,
    cli_permissions: Option<String>,
) -> Result<(String, serde_json::Value), anyhow::Error> {
    let (token_str, permissions_str) = if let Some(url) = url {
        let mut parts = url
            .path_segments()
            .ok_or_else(|| anyhow!("Malformed Tari URL: missing path segments"))?;
        let token = parts
            .next()
            .ok_or_else(|| anyhow!("Malformed Tari URL: missing token segment"))?;
        let permissions_segment = parts
            .next()
            .ok_or_else(|| anyhow!("Malformed Tari URL: missing permissions segment"))?;
        let permissions_decoded = urlencoding::decode(permissions_segment)?;
        (token.to_string(), permissions_decoded.into_owned())
    } else {
        let token = cli_token.ok_or_else(|| {
            anyhow!("Missing signaling server token: pass --url or --signaling-server-token / --token")
        })?;
        let permissions = cli_permissions.ok_or_else(|| {
            anyhow!("Missing WebRTC permissions token: pass --url or --webrtc-permissions-token / --permissions")
        })?;
        (token, permissions)
    };

    let permissions: serde_json::Value = serde_json::from_str(&permissions_str).map_err(|e| {
        anyhow!(
            "Invalid JSON in WebRTC permissions token (via --permissions, --webrtc-permissions-token or --url): {e}"
        )
    })?;

    Ok((token_str, permissions))
}

impl WebRtcSubcommand {
    pub async fn handle(self, mut client: WalletDaemonClient) -> anyhow::Result<()> {
        #[allow(clippy::enum_glob_use)]
        use WebRtcSubcommand::*;
        match self {
            Start(args) => {
                let (signaling_server_token, permissions) = resolve_start_params(
                    args.signaling_server_url.as_ref(),
                    args.signaling_server_token,
                    args.webrtc_permissions_token,
                )?;

                let _resp = client
                    .webrtc_start(WebRtcStartRequest {
                        signaling_server_token,
                        permissions,
                    })
                    .await?;
            },
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct CliTest {
        #[clap(subcommand)]
        command: WebRtcSubcommand,
    }

    #[test]
    fn test_start_args_flags_parsing() {
        let cli = CliTest::try_parse_from([
            "wallet",
            "start",
            "--token",
            "test_token_123",
            "--permissions",
            "{\"test\": true}",
        ])
        .expect("Failed to parse webrtc start flags");

        match cli.command {
            WebRtcSubcommand::Start(args) => {
                assert_eq!(args.signaling_server_token.as_deref(), Some("test_token_123"));
                assert_eq!(args.webrtc_permissions_token.as_deref(), Some("{\"test\": true}"));
                assert_eq!(args.signaling_server_url, None);
            },
        }
    }

    #[test]
    fn test_start_args_url_parsing() {
        let cli = CliTest::try_parse_from([
            "wallet",
            "start",
            "--url",
            "https://signaling.tari.com/tok123/%7B%22foo%22%3A%22bar%22%7D",
        ])
        .expect("Failed to parse webrtc start url");

        match cli.command {
            WebRtcSubcommand::Start(args) => {
                assert!(args.signaling_server_url.is_some());
            },
        }
    }

    #[test]
    fn test_resolve_start_params_valid_url() {
        let url: Url = "https://signaling.tari.com/tok123/%7B%22foo%22%3A%22bar%22%7D"
            .parse()
            .unwrap();
        let (token, permissions) = resolve_start_params(Some(&url), None, None).unwrap();
        assert_eq!(token, "tok123");
        assert_eq!(permissions, serde_json::json!({"foo": "bar"}));
    }

    #[test]
    fn test_resolve_start_params_url_missing_segment() {
        let url: Url = "https://signaling.tari.com/tok123".parse().unwrap();
        let res = resolve_start_params(Some(&url), None, None);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("missing permissions segment"));
    }

    #[test]
    fn test_resolve_start_params_invalid_json() {
        let res = resolve_start_params(None, Some("token123".to_string()), Some("not_valid_json".to_string()));
        assert!(res.is_err());
        assert!(
            res.unwrap_err()
                .to_string()
                .contains("Invalid JSON in WebRTC permissions token")
        );
    }

    #[test]
    fn test_resolve_start_params_neither_url_nor_flags() {
        let res = resolve_start_params(None, None, None);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("Missing signaling server token"));
    }
}
