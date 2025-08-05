//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use borsh::BorshSerialize;
use tari_bor::{Deserialize, Serialize};
use tari_crypto::ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey};
use tari_template_lib::{models::EncryptedData, prelude::RistrettoPublicKeyBytes};

use crate::{
    crypto::{CompressedElgamalVerifiableBalance, ElgamalVerifiableBalance},
    ToByteType,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct PrivateOutput {
    pub public_nonce: RistrettoPublicKeyBytes,
    pub encrypted_data: EncryptedData,
    #[cfg_attr(feature = "ts", ts(type = "number | bigint"))]
    pub minimum_value_promise: u64,
    pub viewable_balance: Option<CompressedElgamalVerifiableBalance>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPrivateOutput {
    pub commitment: PedersenCommitment,
    pub public_nonce: RistrettoPublicKey,
    pub encrypted_data: EncryptedData,
    pub minimum_value_promise: u64,
    pub viewable_balance: Option<ElgamalVerifiableBalance>,
}

impl ValidatedPrivateOutput {
    pub fn to_private_output(&self) -> PrivateOutput {
        PrivateOutput {
            public_nonce: self.public_nonce.to_byte_type(),
            encrypted_data: self.encrypted_data.clone(),
            minimum_value_promise: self.minimum_value_promise,
            viewable_balance: self.viewable_balance.as_ref().map(|b| b.to_byte_type()),
        }
    }
}
