//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_bor::{Deserialize, Serialize};
use tari_crypto::ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey};
use tari_template_lib::{models::EncryptedData, prelude::RistrettoPublicKeyBytes};

use crate::{
    crypto::{CompressedElgamalVerifiableBalance, ElgamalVerifiableBalance},
    ToByteType,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct PrivateOutput {
    pub stealth_public_nonce: RistrettoPublicKeyBytes,
    pub encrypted_data: EncryptedData,
    #[cfg_attr(feature = "ts", ts(type = "number | bigint"))]
    pub minimum_value_promise: u64,
    pub viewable_balance: Option<CompressedElgamalVerifiableBalance>,
}

impl From<ValidatedPrivateOutput> for PrivateOutput {
    fn from(output: ValidatedPrivateOutput) -> Self {
        Self {
            stealth_public_nonce: output.stealth_public_nonce.to_byte_type(),
            encrypted_data: output.encrypted_data,
            minimum_value_promise: output.minimum_value_promise,
            viewable_balance: output.viewable_balance.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPrivateOutput {
    pub commitment: PedersenCommitment,
    pub stealth_public_nonce: RistrettoPublicKey,
    pub encrypted_data: EncryptedData,
    pub minimum_value_promise: u64,
    pub viewable_balance: Option<ElgamalVerifiableBalance>,
}
