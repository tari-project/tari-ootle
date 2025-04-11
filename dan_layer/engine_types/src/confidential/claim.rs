//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_lib::{
    models::{ConfidentialWithdrawProof, UnclaimedConfidentialOutputAddress},
    prelude::RistrettoPublicKeyBytes,
    types::{crypto::CommitmentSignatureBytes, serde_helpers},
};

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct ConfidentialClaim {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub public_key: RistrettoPublicKeyBytes,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub output_address: UnclaimedConfidentialOutputAddress,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[serde(with = "serde_helpers::dynamic_hex")]
    pub range_proof: Vec<u8>,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub proof_of_knowledge: CommitmentSignatureBytes,
    pub withdraw_proof: Option<ConfidentialWithdrawProof>,
}
