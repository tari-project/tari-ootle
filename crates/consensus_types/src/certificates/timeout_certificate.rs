//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Display, hash::Hash};

use borsh::BorshSerialize;
use serde::{Deserialize, Serialize};
use tari_ootle_common_types::{hashing::timeout_certificate_id_hasher, Epoch, NodeHeight};

use crate::{validator_signature::ValidatorSignatureBytes, HighTc, TcId};

#[derive(Debug, Clone, Hash, Deserialize, Serialize, BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TimeoutCertificate {
    epoch: Epoch,
    height: NodeHeight,
    /// A quorum of validator signatures that sign the timeout certificate.
    signatures: Vec<ValidatorSignatureBytes>,
}

impl TimeoutCertificate {
    pub fn new(epoch: Epoch, height: NodeHeight, signatures: Vec<ValidatorSignatureBytes>) -> Self {
        Self {
            epoch,
            height,
            signatures,
        }
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    pub fn height(&self) -> NodeHeight {
        self.height
    }

    pub fn signatures(&self) -> &[ValidatorSignatureBytes] {
        &self.signatures
    }

    pub fn calculate_id(&self) -> TcId {
        timeout_certificate_id_hasher().chain(self).finalize_into_array().into()
    }

    pub fn as_high_tc(&self) -> HighTc {
        HighTc {
            epoch: self.epoch,
            height: self.height,
            tc_id: self.calculate_id(),
        }
    }
}

impl Display for TimeoutCertificate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TimeoutCertificate {{ epoch: {}, height: {}, signatures: {} }}",
            self.epoch,
            self.height,
            self.signatures.len()
        )
    }
}
