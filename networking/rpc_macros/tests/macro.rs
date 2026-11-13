//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Compile-time fixture for the code `#[tari_rpc]` generates.

use tari_rpc_framework::{Request, Response, RpcRequestOptions, RpcStatus, Streaming};
use tari_rpc_macros::tari_rpc;

#[derive(Clone, PartialEq, prost::Message)]
pub struct TestMessage {
    #[prost(uint32, tag = "1")]
    pub value: u32,
}

#[tari_rpc(protocol_name = "/test/macro/1.0", server_struct = TestRpcServer, client_struct = TestRpcClient)]
pub trait TestRpcService: Send + Sync + 'static {
    #[rpc(method = 1)]
    async fn unary(&self, request: Request<TestMessage>) -> Result<Response<TestMessage>, RpcStatus>;

    #[rpc(method = 2)]
    async fn streaming(&self, request: Request<TestMessage>) -> Result<Streaming<TestMessage>, RpcStatus>;

    #[rpc(method = 3)]
    async fn unit_streaming(&self, request: Request<()>) -> Result<Streaming<TestMessage>, RpcStatus>;

    #[rpc(method = 4)]
    async fn qualified_types(
        &self,
        request: tari_rpc_framework::Request<TestMessage>,
    ) -> std::result::Result<tari_rpc_framework::Response<TestMessage>, tari_rpc_framework::RpcStatus>;
}

/// Streaming methods are generated with a variant that takes per-request options, including one
/// whose request type is the unit type; unary methods are served with the session's options and
/// have no such variant.
#[test]
fn streaming_methods_are_generated_with_a_request_options_variant() {
    async fn call_each(client: &mut TestRpcClient, options: RpcRequestOptions) {
        let _stream = client.streaming_with_options(TestMessage::default(), options).await;
        let _stream = client.unit_streaming_with_options(options).await;
        let _response = client.unary(TestMessage::default()).await;
        let _response = client.qualified_types(TestMessage::default()).await;
    }

    let _ = call_each;
}
