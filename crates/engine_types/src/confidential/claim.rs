//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_lib::{
    models::UnclaimedConfidentialOutputAddress,
    prelude::{RistrettoPublicKeyBytes, StealthTransferStatement},
    types::crypto::{CommitmentSignatureBytes, RangeProofBytes},
};

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TariStealthClaim {
    /// This is typically the public nonce that the UTXO was burnt with
    pub burn_public_key: RistrettoPublicKeyBytes,
    pub output_address: UnclaimedConfidentialOutputAddress,
    pub range_proof: RangeProofBytes,
    pub proof_of_knowledge: CommitmentSignatureBytes,
    pub transfer_statement: Option<StealthTransferStatement>,
}
