//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_walletd_client::types::ClaimBurnProof;
use tari_sidechain::CompleteClaimBurnProof;
use tari_template_lib_types::crypto::PedersenCommitmentBytes;

pub enum CucumberClaimProof {
    Confirmed {
        proof: ClaimBurnProof,
        /// The same proof in the on-disk file format, used by the auto-claim integration tests.
        complete_proof: Box<CompleteClaimBurnProof>,
    },
    Pending {
        commitment: PedersenCommitmentBytes,
        kernel_excess_sig_nonce: Vec<u8>,
        kernel_excess_sig_signature: Vec<u8>,
    },
}

impl CucumberClaimProof {
    pub fn confirmed(&self) -> Option<&ClaimBurnProof> {
        match self {
            Self::Confirmed { proof, .. } => Some(proof),
            _ => None,
        }
    }

    pub fn complete_proof(&self) -> Option<&CompleteClaimBurnProof> {
        match self {
            Self::Confirmed { complete_proof, .. } => Some(complete_proof),
            _ => None,
        }
    }
}
