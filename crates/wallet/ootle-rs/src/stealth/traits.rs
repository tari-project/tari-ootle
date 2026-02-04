//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use async_trait::async_trait;
use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSecretKey};
use tari_ootle_common_types::engine_types::crypto::OutputBody;
use tari_ootle_wallet_crypto::DecryptedData;
use tari_template_lib_types::{Amount, crypto::PedersenCommitmentBytes, stealth::StealthOutputsStatement};

use crate::stealth::{Output, error::StealthProviderError};

pub type StealthResult<T> = Result<T, StealthProviderError>;

#[async_trait]
pub trait StealthOutputStatementFactory {
    async fn generate_outputs_statement(
        &self,
        specs: Vec<Output>,
        revealed_output_amount: Amount,
    ) -> StealthResult<(StealthOutputsStatement, RistrettoSecretKey)>;
}

#[async_trait]
pub trait InputDecryptor {
    async fn decrypt_input_data(
        &self,
        commitment: &PedersenCommitmentBytes,
        input: &OutputBody,
        skip_memo: bool,
    ) -> StealthResult<DecryptedData>;
}

pub trait StealthSigner {
    type Signature;

    fn sign_with_stealth_key(&self, public_key: &RistrettoPublicKey) -> Result<Self::Signature, String>;
}

pub trait StealthProvider: StealthOutputStatementFactory + InputDecryptor {}
impl<T> StealthProvider for T where T: StealthOutputStatementFactory + InputDecryptor {}
