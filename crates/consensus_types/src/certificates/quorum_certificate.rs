//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use borsh::BorshSerialize;
use serde::{Deserialize, Serialize};
use tari_ootle_common_types::{Epoch, NodeHeight};

use crate::{ProposalCertificate, TimeoutCertificate};

#[derive(Debug, Clone, Deserialize, Serialize, BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum QuorumCertificate {
    ProposalCertificate(ProposalCertificate),
    TimeoutCertificate(TimeoutCertificate),
}

impl QuorumCertificate {
    pub fn is_proposal_certificate(&self) -> bool {
        matches!(self, Self::ProposalCertificate(_))
    }

    pub fn is_timeout_certificate(&self) -> bool {
        matches!(self, Self::TimeoutCertificate(_))
    }

    pub fn type_str(&self) -> &'static str {
        match self {
            Self::ProposalCertificate(_) => "ProposalCertificate",
            Self::TimeoutCertificate(_) => "TimeoutCertificate",
        }
    }

    pub fn epoch(&self) -> Epoch {
        match self {
            Self::ProposalCertificate(pc) => pc.epoch(),
            Self::TimeoutCertificate(tc) => tc.epoch(),
        }
    }

    pub fn height(&self) -> NodeHeight {
        match self {
            Self::ProposalCertificate(pc) => pc.height(),
            Self::TimeoutCertificate(tc) => tc.height(),
        }
    }

    pub fn as_proposal_certificate(&self) -> Option<&ProposalCertificate> {
        if let Self::ProposalCertificate(pc) = self {
            Some(pc)
        } else {
            None
        }
    }

    pub fn as_timeout_certificate(&self) -> Option<&TimeoutCertificate> {
        if let Self::TimeoutCertificate(tc) = self {
            Some(tc)
        } else {
            None
        }
    }

    pub fn into_proposal_certificate(self) -> Option<ProposalCertificate> {
        if let Self::ProposalCertificate(pc) = self {
            Some(pc)
        } else {
            None
        }
    }

    pub fn into_timeout_certificate(self) -> Option<TimeoutCertificate> {
        if let Self::TimeoutCertificate(tc) = self {
            Some(tc)
        } else {
            None
        }
    }
}
