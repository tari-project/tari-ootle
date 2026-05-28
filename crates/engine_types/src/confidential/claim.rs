//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display};

use serde::{Deserialize, Serialize};
use tari_template_lib::types::{
    EncryptedData,
    Hash32,
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, SchnorrSignatureBytes},
};

#[derive(
    Debug,
    Clone,
    Deserialize,
    Serialize,
    PartialEq,
    borsh::BorshSerialize,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct MinotariBurnClaimProof {
    /// This is typically the public nonce that the UTXO was burnt with
    #[n(0)]
    pub burn_public_key: RistrettoPublicKeyBytes,
    #[n(1)]
    pub commitment: PedersenCommitmentBytes,
    #[n(2)]
    pub ownership_proof: SchnorrSignatureBytes,
    #[n(3)]
    pub encoded_merkle_proof: EncodedMerkleProof,
    #[n(4)]
    pub kernel: AbridgedTransactionKernel,
    #[n(5)]
    pub value: u64,
    #[n(6)]
    pub sender_offset_public_key: RistrettoPublicKeyBytes,
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
            self.value,
        )
    }
}

#[derive(
    Debug,
    Clone,
    Deserialize,
    Serialize,
    PartialEq,
    borsh::BorshSerialize,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ClaimBurnOutputData {
    #[n(0)]
    pub encrypted_data: EncryptedData,
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    borsh::BorshSerialize,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct EncodedMerkleProof {
    #[n(0)]
    #[borsh(serialize_with = "serialize_bytes")]
    #[serde(with = "ootle_serde::base64")]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub block_hash: Hash32,
    #[n(1)]
    #[serde(with = "ootle_serde::base64")]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[borsh(serialize_with = "serialize_bytes")]
    #[cbor(with = "bounded_vec_bytes")]
    pub encoded_merkle_proof: bounded_vec::BoundedVec<u8, 1, 4096>,
    #[n(2)]
    pub leaf_index: u64,
}

/// Adapter for `bounded_vec::BoundedVec<u8, LOW, HIGH>` so it can participate in minicbor derives.
/// On the wire encodes as a CBOR byte string (matches the canonical bytes encoding).
mod bounded_vec_bytes {
    use bounded_vec::BoundedVec;
    use minicbor::{CborLen, Decoder, Encoder};

    pub fn encode<C, W, const LOW: usize, const HIGH: usize>(
        v: &BoundedVec<u8, LOW, HIGH>,
        e: &mut Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>>
    where
        W: minicbor::encode::Write,
    {
        e.bytes(v.as_slice())?;
        Ok(())
    }

    pub fn decode<'b, C, const LOW: usize, const HIGH: usize>(
        d: &mut Decoder<'b>,
        _ctx: &mut C,
    ) -> Result<BoundedVec<u8, LOW, HIGH>, minicbor::decode::Error> {
        let bytes = d.bytes()?;
        <BoundedVec<u8, LOW, HIGH>>::from_vec(bytes.to_vec())
            .map_err(|_| minicbor::decode::Error::message("BoundedVec length out of bounds"))
    }

    pub fn cbor_len<C, const LOW: usize, const HIGH: usize>(v: &BoundedVec<u8, LOW, HIGH>, ctx: &mut C) -> usize {
        <[u8] as CborLen<C>>::cbor_len(v.as_slice(), ctx)
    }
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    borsh::BorshSerialize,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AbridgedTransactionKernel {
    #[n(0)]
    pub version: u8,
    #[n(1)]
    pub fee: u64,
    #[n(2)]
    pub lock_height: u64,
    #[n(3)]
    pub excess: PedersenCommitmentBytes,
    #[n(4)]
    pub excess_sig: SchnorrSignatureBytes,
}

pub(crate) fn serialize_bytes<W: borsh::io::Write, T: AsRef<[u8]>>(
    obj: &T,
    writer: &mut W,
) -> Result<(), borsh::io::Error> {
    borsh::BorshSerialize::serialize(obj.as_ref(), writer)
}
