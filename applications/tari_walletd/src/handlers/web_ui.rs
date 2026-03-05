//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

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
    <title>Tari Wallet Daemon</title>
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

#[cfg(feature = "web_ui")]
mod enabled {
    use std::str::FromStr;

    use include_dir::include_dir;

    use super::*;

    static PROJECT_DIR: include_dir::Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/web_ui/dist");

    pub async fn handler(uri: Uri) -> impl IntoResponse {
        let path = uri.path();

        // If path starts with /, strip it.
        let path = path.strip_prefix('/').unwrap_or(path);

        // If the path is a file, return it. Otherwise use index.html (SPA)
        if let Some(body) = PROJECT_DIR
            .get_file(path)
            .or_else(|| PROJECT_DIR.get_file("index.html"))
            .and_then(|file| file.contents_utf8())
        {
            let mime_type =
                mime_guess::from_path(path).first_or_else(|| mime_guess::Mime::from_str("text/html").unwrap());
            let content_type = HeaderValue::from_str(mime_type.as_ref()).unwrap_or_else(|e| {
                eprintln!("Error parsing MIME type: {}, defaulting to 'text/html'", e);
                HeaderValue::from_static("text/html")
            });
            return Response::builder()
                .header(header::CONTENT_TYPE, content_type)
                .status(StatusCode::OK)
                .body(body.to_owned())
                .unwrap();
        }
        Response::builder()
            .header(header::CONTENT_TYPE, HeaderValue::from_static("text/html"))
            .status(StatusCode::OK)
            .body(default_page(
                "Web UI Build Failed",
                "The web UI failed to build during compilation. The JSON-RPC API is still available. To fix this, \
                 ensure <code>pnpm</code> is installed and run a release build.",
            ))
            .unwrap()
    }
}

#[cfg(feature = "web_ui")]
pub use enabled::handler;

#[cfg_attr(feature = "web_ui", allow(dead_code))]
pub async fn feature_disabled_handler(_uri: Uri) -> impl IntoResponse {
    Response::builder()
        .header(header::CONTENT_TYPE, HeaderValue::from_static("text/html"))
        .status(StatusCode::OK)
        .body(default_page(
            "Web UI Not Enabled",
            "The wallet daemon was compiled without the <code>web_ui</code> feature. The JSON-RPC API is still \
             available. To enable the web UI, rebuild with <code>cargo build --release --features web_ui</code>.",
        ))
        .unwrap()
}
