//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::RistrettoSecretKey;
use tari_template_lib::types::Amount;

#[derive(Debug, Clone)]
pub struct ConfidentialOutputMaskAndValue {
    pub value: Amount,
    pub mask: RistrettoSecretKey,
}
