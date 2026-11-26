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

    use axum::body::Body;
    use include_dir::{Dir, include_dir};

    use super::*;

    static PROJECT_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/web_ui/dist");

    pub async fn handler(uri: Uri) -> impl IntoResponse {
        serve_file(&PROJECT_DIR, uri.path())
    }

    fn serve_file(dir: &'static Dir<'static>, path: &str) -> Response<Body> {
        let path = path.strip_prefix('/').unwrap_or(path);
        let resolved = match dir.get_file(path) {
            Some(file) => Some((file, path)),
            None => dir.get_file("index.html").map(|file| (file, "index.html")),
        };

        if let Some((file, path)) = resolved {
            let mime_type =
                mime_guess::from_path(path).first_or_else(|| mime_guess::Mime::from_str("text/html").unwrap());
            let content_type = HeaderValue::from_str(mime_type.as_ref()).unwrap_or_else(|e| {
                eprintln!("Error parsing MIME type: {}, defaulting to 'text/html'", e);
                HeaderValue::from_static("text/html")
            });
            return Response::builder()
                .header(header::CONTENT_TYPE, content_type)
                .status(StatusCode::OK)
                .body(Body::from(file.contents()))
                .unwrap();
        }
        Response::builder()
            .header(header::CONTENT_TYPE, HeaderValue::from_static("text/html"))
            .status(StatusCode::OK)
            .body(Body::from(default_page(
                "Web UI Build Failed",
                "The web UI failed to build during compilation. The JSON-RPC API is still available. To fix this, \
                 ensure <code>pnpm</code> is installed and run a release build.",
            )))
            .unwrap()
    }

    #[cfg(test)]
    mod tests {
        use axum::body::to_bytes;
        use include_dir::{DirEntry, File};

        use super::*;

        const WASM: &[u8] = b"\0asm\x01\0\0\0\xff\xfe";

        static DIR: Dir<'static> = Dir::new("", &[
            DirEntry::File(File::new("index.html", b"<html></html>")),
            DirEntry::Dir(Dir::new("assets", &[DirEntry::File(File::new(
                "assets/decoder.wasm",
                WASM,
            ))])),
        ]);

        async fn body(response: Response<Body>) -> Vec<u8> {
            to_bytes(response.into_body(), usize::MAX).await.unwrap().to_vec()
        }

        #[tokio::test]
        async fn a_binary_asset_is_served_as_is() {
            let response = serve_file(&DIR, "/assets/decoder.wasm");

            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CONTENT_TYPE], "application/wasm");
            assert_eq!(body(response).await, WASM);
        }

        #[tokio::test]
        async fn an_unknown_path_is_served_the_index() {
            let response = serve_file(&DIR, "/accounts/account.1");

            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html");
            assert_eq!(body(response).await, b"<html></html>");
        }
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
