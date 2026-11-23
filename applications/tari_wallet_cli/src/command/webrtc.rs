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
    #[clap(required_unless_present = "signaling_server_url")]
    pub signaling_server_token: Option<String>,
    #[clap(required_unless_present = "signaling_server_url")]
    pub webrtc_permissions_token: Option<serde_json::Value>,
}

impl WebRtcSubcommand {
    pub async fn handle(self, mut client: WalletDaemonClient) -> anyhow::Result<()> {
        #[allow(clippy::enum_glob_use)]
        use WebRtcSubcommand::*;
        match self {
            Start(args) => {
                let request = build_start_request(args)?;
                let _resp = client.webrtc_start(request).await?;
            },
        }
        Ok(())
    }
}

fn build_start_request(mut args: StartArgs) -> anyhow::Result<WebRtcStartRequest> {
    if let Some(url) = args.signaling_server_url {
        let mut parts = url.path_segments().ok_or_else(|| anyhow!("Malformed Tari URL"))?;
        args.signaling_server_token = Some(
            parts
                .next()
                .filter(|segment| !segment.is_empty())
                .ok_or_else(|| anyhow!("Malformed Tari URL"))?
                .to_string(),
        );

        let token = parts.next().ok_or_else(|| anyhow!("Malformed Tari URL"))?;
        let token = urlencoding::decode(token)?;
        args.webrtc_permissions_token = Some(serde_json::from_str(token.as_ref())?);
    }

    let signaling_server_token = args
        .signaling_server_token
        .ok_or_else(|| anyhow!("Missing signaling server token (pass --url or signaling_server_token)"))?;
    let permissions = args.webrtc_permissions_token.ok_or_else(|| {
        anyhow!("Missing WebRTC permissions token (pass --url or webrtc_permissions_token)")
    })?;

    Ok(WebRtcStartRequest {
        signaling_server_token,
        permissions,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn build_start_request_errors_when_tokens_missing() {
        let err = build_start_request(StartArgs {
            signaling_server_url: None,
            signaling_server_token: None,
            webrtc_permissions_token: None,
        })
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Missing signaling server token"));
        assert!(msg.contains("signaling_server_token"));

        let err = build_start_request(StartArgs {
            signaling_server_url: None,
            signaling_server_token: Some("sig-token".to_string()),
            webrtc_permissions_token: None,
        })
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Missing WebRTC permissions token"));
        assert!(msg.contains("webrtc_permissions_token"));
    }

    #[test]
    fn build_start_request_accepts_direct_tokens_and_url() {
        let req = build_start_request(StartArgs {
            signaling_server_url: None,
            signaling_server_token: Some("sig-token".to_string()),
            webrtc_permissions_token: Some(json!(["Admin"])),
        })
        .unwrap();
        assert_eq!(req.signaling_server_token, "sig-token");
        assert_eq!(req.permissions, json!(["Admin"]));

        let url = Url::parse("https://signal.example.com/sig-123/%5B%22Admin%22%5D").unwrap();
        let from_url = build_start_request(StartArgs {
            signaling_server_url: Some(url),
            signaling_server_token: None,
            webrtc_permissions_token: None,
        })
        .unwrap();
        assert_eq!(from_url.signaling_server_token, "sig-123");
        assert_eq!(from_url.permissions, json!(["Admin"]));
    }

    #[test]
    fn build_start_request_rejects_empty_first_url_segment() {
        let url = Url::parse("https://signal.example.com//%5B%22Admin%22%5D").unwrap();
        let err = build_start_request(StartArgs {
            signaling_server_url: Some(url),
            signaling_server_token: None,
            webrtc_permissions_token: None,
        })
        .unwrap_err();
        assert!(err.to_string().contains("Malformed Tari URL"));
    }
}
