//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display};

use serde::{Deserialize, Serialize};
use tari_common_types::types::FixedHash;
use tari_template_lib::{
    prelude::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, SchnorrSignatureBytes},
    types::EncryptedData,
};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, borsh::BorshSerialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ClaimBurnOutputData {
    pub encrypted_data: EncryptedData,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct EncodedMerkleProof {
    #[serde(with = "ootle_serde::base64")]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[borsh(serialize_with = "serialize_bytes")]
    pub block_hash: FixedHash,
    #[serde(with = "ootle_serde::base64")]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[borsh(serialize_with = "serialize_bytes")]
    pub encoded_merkle_proof: bounded_vec::BoundedVec<u8, 1, 4096>,
    pub leaf_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AbridgedTransactionKernel {
    pub version: u8,
    pub fee: u64,
    pub lock_height: u64,
    pub excess: PedersenCommitmentBytes,
    pub excess_sig: SchnorrSignatureBytes,
}

pub(crate) fn serialize_bytes<W: borsh::io::Write, T: AsRef<[u8]>>(
    obj: &T,
    writer: &mut W,
) -> Result<(), borsh::io::Error> {
    borsh::BorshSerialize::serialize(obj.as_ref(), writer)
}
