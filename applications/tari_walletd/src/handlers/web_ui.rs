//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use axum::{
    http::{HeaderValue, Response, StatusCode, Uri, header},
    response::IntoResponse,
};
use include_dir::include_dir;

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
        let mime_type = mime_guess::from_path(path).first_or_else(|| mime_guess::Mime::from_str("text/html").unwrap());
        let content_type = mime_type.to_string();
        // let content_type
        return Response::builder()
            .header(header::CONTENT_TYPE, HeaderValue::from_str(&content_type).unwrap())
            .status(StatusCode::OK)
            .body(body.to_owned())
            .unwrap();
    }
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body("".to_string())
        .unwrap()
}
