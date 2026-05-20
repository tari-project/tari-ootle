//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_consensus_types::{ProposalCertificate, TimeoutCertificate};

use crate::traits::Versioned;

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
pub enum VersionedProposalCertificate {
    #[n(0)]
    V1(#[n(0)] ProposalCertificate),
}

impl Versioned for VersionedProposalCertificate {
    type Latest = ProposalCertificate;

    fn upgrade_single_step(self) -> (Self, bool) {
        match self {
            Self::V1(_) => (self, false), // No upgrades available
        }
    }

    fn into_latest(self) -> Self::Latest {
        match self {
            Self::V1(cert) => cert,
        }
    }
}

impl From<ProposalCertificate> for VersionedProposalCertificate {
    fn from(cert: ProposalCertificate) -> Self {
        Self::V1(cert)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
pub enum VersionedTimeoutCertificate {
    #[n(0)]
    V1(#[n(0)] TimeoutCertificate),
}

impl Versioned for VersionedTimeoutCertificate {
    type Latest = TimeoutCertificate;

    fn upgrade_single_step(self) -> (Self, bool) {
        match self {
            Self::V1(_) => (self, false), // No upgrades available
        }
    }

    fn into_latest(self) -> Self::Latest {
        match self {
            Self::V1(cert) => cert,
        }
    }
}

impl From<TimeoutCertificate> for VersionedTimeoutCertificate {
    fn from(cert: TimeoutCertificate) -> Self {
        Self::V1(cert)
    }
}
