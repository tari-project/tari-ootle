//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod accounts;
pub mod auth;
pub mod confidential;
mod context;
pub mod error;
mod helpers;
pub mod keys;
pub mod nfts;
pub mod rpc;
pub mod settings;
pub mod stealth_utxos;
pub mod substates;
pub mod templates;
pub mod transaction;
pub mod validator;
pub mod wallet;
pub mod wasm_optimizer;
pub mod webauthn;
pub mod webrtc;

use std::future::Future;

use async_trait::async_trait;
use axum_extra::headers::authorization::Bearer;
pub use context::HandlerContext;
use error::HandlerError;

// NOTE: async_trait is needed even with stabilization of async traits due to this Rust limitation: https://github.com/rust-lang/rust/issues/100013
#[async_trait]
pub trait Handler<'a, TReq> {
    type Response;

    async fn handle(
        &mut self,
        context: &'a HandlerContext,
        token: Option<&'a Bearer>,
        req: TReq,
    ) -> Result<Self::Response, HandlerError>;
}

#[async_trait]
impl<'a, F, TReq, TResp, TFut, TErr> Handler<'a, TReq> for F
where
    F: FnMut(&'a HandlerContext, Option<&'a Bearer>, TReq) -> TFut + Sync + Send,
    TFut: Future<Output = Result<TResp, TErr>> + Send,
    TReq: Send + 'static,
    TErr: Into<HandlerError>,
{
    type Response = TResp;

    async fn handle(
        &mut self,
        context: &'a HandlerContext,
        token: Option<&'a Bearer>,
        req: TReq,
    ) -> Result<Self::Response, HandlerError> {
        let resp = self(context, token, req).await.map_err(Into::into)?;
        Ok(resp)
    }
}
