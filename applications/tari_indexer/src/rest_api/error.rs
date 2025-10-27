//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{borrow::Cow, env};

use axum::{http::StatusCode, response::IntoResponse, Json};
use log::*;
use serde_json::json;
use utoipa::{
    openapi::{RefOr, Schema},
    PartialSchema,
    ToSchema,
};

const LOG_TARGET: &str = "tari::indexer::rest_api::error";

#[macro_export]
macro_rules! bailout {
    ($msg:literal $(,)?) => {
        {
            let error = $crate::rest_api::error::ErrorResponse::anyhow(anyhow::format_err!($msg));
            return Err(error);
        }
    };
    ($fmt:expr, $($arg:tt)*) => {
        return Err($crate::rest_api::error::ErrorResponse::anyhow(anyhow::Error::msg(format!($fmt, $($arg)*))));
    };
}

#[derive(Debug, Clone)]
pub struct ErrorResponse {
    pub status: StatusCode,
    pub error: Box<str>,
}

#[derive(ToSchema)]
struct ErrorResp {
    #[allow(unused)]
    pub error: Box<str>,
}

impl ErrorResponse {
    #[must_use]
    pub fn anyhow<E: Into<anyhow::Error>>(err: E) -> Self {
        let err = err.into();
        error!(target: LOG_TARGET, "Internal server error: {}", err);
        Self::internal_error(err.to_string())
    }

    #[must_use]
    pub fn internal_error(msg: impl Into<Box<str>>) -> Self {
        let msg = msg.into();
        error!(target: LOG_TARGET, "Internal server error: {}", msg);
        let msg = if cfg!(debug_assertions) || env::var("API_DEBUG").ok().is_some_and(|v| v != "0") {
            msg
        } else {
            "Internal server error".into()
        };
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: msg,
        }
    }

    pub fn general_error(msg: impl Into<Box<str>>) -> Self {
        let msg = msg.into();
        error!(target: LOG_TARGET, "General error: {}", msg);
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: msg,
        }
    }

    #[must_use]
    pub fn not_found(msg: impl Into<Box<str>>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            error: msg.into(),
        }
    }

    #[must_use]
    pub fn bad_request(msg: impl Into<Box<str>>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: msg.into(),
        }
    }

    #[must_use]
    pub fn service_unavailable(msg: impl Into<Box<str>>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            error: msg.into(),
        }
    }
}

impl From<anyhow::Error> for ErrorResponse {
    fn from(err: anyhow::Error) -> Self {
        Self::anyhow(err)
    }
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(json!({"error": self.error}))).into_response()
    }
}

impl PartialSchema for ErrorResponse {
    fn schema() -> RefOr<Schema> {
        ErrorResp::schema()
    }
}

impl ToSchema for ErrorResponse {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("ErrorResponse")
    }
}
