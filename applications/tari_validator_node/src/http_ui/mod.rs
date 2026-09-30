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

//! Serves the embedded Web UI from the JSON-RPC listener.

use axum::{
    http::{HeaderValue, Response, StatusCode, Uri, header},
    response::IntoResponse,
};

fn default_page(title: &str, message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Tari Validator Node</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; display: flex; justify-content: center; align-items: center; min-height: 100vh; margin: 0; background: #1a1a2e; color: #e0e0e0; }}
        .container {{ text-align: center; max-width: 500px; padding: 2rem; }}
        h1 {{ color: #9b59b6; }}
        p {{ line-height: 1.6; color: #b0b0b0; }}
        code {{ background: #2d2d44; padding: 0.2em 0.5em; border-radius: 4px; font-size: 0.9em; }}
    </style>
</head>
<body>
    <div class="container">
        <h1>{title}</h1>
        <p>{message}</p>
    </div>
</body>
</html>"#
    )
}

fn html_response(body: String) -> Response<String> {
    Response::builder()
        .header(header::CONTENT_TYPE, HeaderValue::from_static("text/html"))
        .status(StatusCode::OK)
        .body(body)
        .unwrap()
}

#[cfg(feature = "web_ui")]
mod enabled {
    use std::str::FromStr;

    use include_dir::{Dir, include_dir};

    use super::*;

    static PROJECT_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/web_ui/dist");

    pub async fn handler(uri: Uri) -> impl IntoResponse {
        let path = uri.path();
        let path = path.strip_prefix('/').unwrap_or(path);

        // Unknown paths get index.html so that the SPA router can resolve them
        if let Some(body) = PROJECT_DIR
            .get_file(path)
            .or_else(|| PROJECT_DIR.get_file("index.html"))
            .and_then(|file| file.contents_utf8())
        {
            let mime_type =
                mime_guess::from_path(path).first_or_else(|| mime_guess::Mime::from_str("text/html").unwrap());
            let content_type =
                HeaderValue::from_str(mime_type.as_ref()).unwrap_or_else(|_| HeaderValue::from_static("text/html"));
            return Response::builder()
                .header(header::CONTENT_TYPE, content_type)
                .status(StatusCode::OK)
                .body(body.to_owned())
                .unwrap();
        }
        html_response(default_page(
            "Web UI Build Failed",
            "The web UI failed to build during compilation. The JSON-RPC API is still available. To fix this, ensure \
             <code>pnpm</code> is installed and run a release build.",
        ))
    }
}

#[cfg(feature = "web_ui")]
pub use enabled::handler;

#[cfg(not(feature = "web_ui"))]
pub async fn handler(_uri: Uri) -> impl IntoResponse {
    html_response(default_page(
        "Web UI Not Enabled",
        "The validator node was compiled without the <code>web_ui</code> feature. The JSON-RPC API is still \
         available. To enable the web UI, rebuild with <code>cargo build --release --features web_ui</code>.",
    ))
}
