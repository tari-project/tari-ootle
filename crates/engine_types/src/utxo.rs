//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_bor::{Deserialize, Serialize};
use tari_template_lib::types::{
    crypto::{RistrettoPublicKeyBytes, UtxoTag},
    stealth::SpendAuthorization,
};

use crate::crypto::OutputBody;

#[derive(
    Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, Serialize, Deserialize, borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Utxo {
    #[n(0)]
    pub output: Option<UtxoOutput>,
    #[n(1)]
    pub is_frozen: bool,
}

impl Utxo {
    pub fn new(output: UtxoOutput) -> Self {
        Self {
            output: Some(output),
            is_frozen: false,
        }
    }

    pub fn output(&self) -> Option<&UtxoOutput> {
        self.output.as_ref()
    }

    pub fn into_output(self) -> Option<UtxoOutput> {
        self.output
    }

    pub fn spender_public_key(&self) -> Option<RistrettoPublicKeyBytes> {
        self.output().and_then(|o| o.auth.spend_key())
    }

    pub fn freeze(&mut self) {
        self.is_frozen = true;
    }

    pub fn unfreeze(&mut self) {
        self.is_frozen = false;
    }

    pub fn burn(&mut self) {
        self.output = None;
    }

    pub fn is_frozen(&self) -> bool {
        self.is_frozen
    }

    pub fn is_burnt(&self) -> bool {
        self.output.is_none()
    }

    /// Returns the UTXO’s tag byte if the UTXO has not been burnt.
    pub fn tag(&self) -> Option<UtxoTag> {
        self.output.as_ref().map(|o| o.tag)
    }
}

#[derive(
    Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, Serialize, Deserialize, borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoOutput {
    #[n(0)]
    pub output: OutputBody,
    #[n(1)]
    pub auth: SpendAuthorization,
    #[n(2)]
    pub tag: UtxoTag,
}

impl UtxoOutput {
    pub fn output(&self) -> &OutputBody {
        &self.output
    }
}
