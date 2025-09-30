//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{env, fmt::Display};

use axum_jrpc::{
    error::{JsonRpcError, JsonRpcErrorReason},
    JsonRpcResponse,
};

const LOG_TARGET: &str = "tari::indexer::json_rpc";

pub fn internal_error<T: Display>(answer_id: axum_jrpc::Id) -> impl Fn(T) -> JsonRpcResponse {
    move |err| {
        log::error!(target: LOG_TARGET, "🚨 Internal error: {}", err);
        let msg = if cfg!(debug_assertions) ||
            env::var("CI").is_ok() ||
            env::var("DEBUG_MODE").ok().as_deref() == Some("1")
        {
            format!("An internal error occurred: {}", err)
        } else {
            "An internal error occurred".to_string()
        };
        JsonRpcResponse::error(
            answer_id.clone(),
            JsonRpcError::new(JsonRpcErrorReason::InternalError, msg, serde_json::Value::Null),
        )
    }
}
