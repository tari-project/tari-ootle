//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display};

use serde::{Deserialize, Serialize};
use tari_common_types::types::FixedHash;
use tari_template_lib::{
    models::EncryptedData,
    prelude::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, SchnorrSignatureBytes},
};

use crate::serde_with;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct MinotariBurnClaimProof {
    /// This is typically the public nonce that the UTXO was burnt with
    pub burn_public_key: RistrettoPublicKeyBytes,
    pub commitment: PedersenCommitmentBytes,
    pub ownership_proof: SchnorrSignatureBytes,
    pub encoded_merkle_proof: EncodedMerkleProof,
    pub kernel: AbridgedTransactionKernel,
    pub value: u64,
}

impl Display for MinotariBurnClaimProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MinotariBurnClaimProof (burn_public_key: {}, commitment: {}, ownership_proof: {}, encoded_merkle_proof: \
             {} bytes, kernel: {}, value: {})",
            self.burn_public_key,
            self.commitment,
            self.ownership_proof,
            self.encoded_merkle_proof.encoded_merkle_proof.len(),
            self.kernel.excess_sig,
            self.value
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ClaimBurnOutputData {
    pub encrypted_data: EncryptedData,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct EncodedMerkleProof {
    #[serde(with = "serde_with::base64")]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub block_hash: FixedHash,
    #[serde(with = "serde_with::base64")]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub encoded_merkle_proof: bounded_vec::BoundedVec<u8, 1, 4096>,
    pub leaf_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AbridgedTransactionKernel {
    pub version: u8,
    pub fee: u64,
    pub lock_height: u64,
    pub excess: PedersenCommitmentBytes,
    pub excess_sig: SchnorrSignatureBytes,
}
