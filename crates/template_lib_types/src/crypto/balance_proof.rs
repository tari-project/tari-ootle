//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::crypto::SchnorrSignatureBytes;

/// The signature of a balance proof, used to validate the authorship of confidential transfers
pub type BalanceProofSignature = SchnorrSignatureBytes;
