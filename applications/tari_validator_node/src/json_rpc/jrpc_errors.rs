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

const LOG_TARGET: &str = "tari::validator_node::json_rpc::handlers";

use std::fmt::Display;

use axum_jrpc::{
    error::{JsonRpcError, JsonRpcErrorReason},
    JsonRpcResponse,
};

// pub fn invalid_params<T: Display, S: Display>(answer_id: axum_jrpc::Id, details: S) -> impl Fn(T) -> JsonRpcResponse
// {     move |err| {
//         log::error!(target: LOG_TARGET, "⚠️ Request has invalid params: {details}. Error: {}", err);
//         JsonRpcResponse::error(
//             answer_id,
//             JsonRpcError::new(
//                 JsonRpcErrorReason::InvalidParams,
//                 format!("Invalid params: {}. Error: {}", details, err),
//                 serde_json::Value::Null,
//             ),
//         )
//     }
// }

/// Creates a handler for internal errors. This will log the error and return a generic message to the user.
pub fn internal_error<T: Display>(answer_id: axum_jrpc::Id) -> impl FnOnce(T) -> JsonRpcResponse {
    move |err| {
        log::error!(target: LOG_TARGET, "🚨 Internal error: {}", err);
        let msg = if cfg!(debug_assertions) || option_env!("CI").is_some() || option_env!("DEBUG_MODE") == Some("1") {
            format!("An internal error occurred: {}", err)
        } else {
            "An internal error occurred".to_string()
        };
        JsonRpcResponse::error(
            answer_id,
            JsonRpcError::new(JsonRpcErrorReason::InternalError, msg, serde_json::Value::Null),
        )
    }
}

pub fn not_found<T: Into<String>>(answer_id: axum_jrpc::Id, details: T) -> JsonRpcResponse {
    JsonRpcResponse::error(
        answer_id,
        JsonRpcError::new(
            JsonRpcErrorReason::ApplicationError(404),
            details.into(),
            serde_json::Value::Null,
        ),
    )
}

/// Creates a handler for general errors. The error will be sent to the client as a JSON-RPC error response.
pub fn general_error<T: Into<String>>(answer_id: axum_jrpc::Id, details: T) -> JsonRpcResponse {
    JsonRpcResponse::error(
        answer_id,
        JsonRpcError::new(
            JsonRpcErrorReason::ApplicationError(500),
            details.into(),
            serde_json::Value::Null,
        ),
    )
}

pub fn invalid_operation<T: Into<String>>(answer_id: axum_jrpc::Id, details: T) -> JsonRpcResponse {
    JsonRpcResponse::error(
        answer_id,
        JsonRpcError::new(
            JsonRpcErrorReason::ApplicationError(400),
            details.into(),
            serde_json::Value::Null,
        ),
    )
}
