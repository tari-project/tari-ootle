//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use borsh::BorshSerialize;
use ootle_byte_type::ToByteType;
use tari_bor::{Deserialize, Serialize};
use tari_crypto::ristretto::{RistrettoPublicKey, pedersen::PedersenCommitment};
use tari_template_lib::types::{EncryptedData, crypto::RistrettoPublicKeyBytes};

use crate::crypto::{ElgamalVerifiableBalance, ElgamalVerifiableBalanceBytes};

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
    Serialize,
    Deserialize,
    BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct OutputBody {
    #[n(0)]
    pub public_nonce: RistrettoPublicKeyBytes,
    #[n(1)]
    pub encrypted_data: EncryptedData,
    #[n(2)]
    #[cfg_attr(feature = "ts", ts(type = "number | bigint"))]
    pub minimum_value_promise: u64,
    #[n(3)]
    pub viewable_balance: Option<ElgamalVerifiableBalanceBytes>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidateOutputBody {
    pub commitment: PedersenCommitment,
    pub public_nonce: RistrettoPublicKey,
    pub encrypted_data: EncryptedData,
    pub minimum_value_promise: u64,
    pub viewable_balance: Option<ElgamalVerifiableBalance>,
}

impl ValidateOutputBody {
    pub fn into_output_body(self) -> OutputBody {
        OutputBody {
            public_nonce: self.public_nonce.to_byte_type(),
            encrypted_data: self.encrypted_data,
            minimum_value_promise: self.minimum_value_promise,
            viewable_balance: self.viewable_balance.as_ref().map(|b| b.to_byte_type()),
        }
    }
}
