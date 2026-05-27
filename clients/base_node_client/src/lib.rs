//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod error;
pub use error::BaseNodeClientError;

pub mod grpc;
pub mod types;

mod traits;
pub use ::futures_util;
// Re-exported because they appear in this crate's public API: `ValidatorNodeChange` in the
// `BaseNodeClient` trait signature and `tonic::Code` in `BaseNodeClientError::GrpcStatus`. This lets
// downstream code (e.g. mock clients in tests) name them without depending on minotari_app_grpc/tonic.
pub use ::tonic;
pub use minotari_app_grpc::tari_rpc::ValidatorNodeChange;
pub use traits::BaseNodeClient;
