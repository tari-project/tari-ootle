//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "tari-indexer-client/", rename = "IndexerGetIdentityResponse")
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GetIdentityResponse {
    pub peer_id: String,
    #[cfg_attr(feature = "utoipa", schema(value_type = String))]
    pub public_key: RistrettoPublicKeyBytes,
    #[cfg_attr(feature = "utoipa", schema(value_type = Vec<String>))]
    #[cfg_attr(feature = "ts", ts(type = "string[]"))]
    pub public_addresses: Vec<Multiaddr>,
}
