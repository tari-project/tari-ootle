//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::PedersenCommitmentBytes;
use tari_wallet_daemon_client::types::ClaimBurnProof;

pub enum CucumberClaimProof {
    Confirmed {
        proof: Box<ClaimBurnProof>,
    },
    Pending {
        commitment: PedersenCommitmentBytes,
        nonce_id: u64,
        kernel_excess_sig_nonce: Vec<u8>,
        kernel_excess_sig_signature: Vec<u8>,
    },
}

impl CucumberClaimProof {
    pub fn confirmed(&self) -> Option<&ClaimBurnProof> {
        match self {
            Self::Confirmed { proof } => Some(proof),
            _ => None,
        }
    }
}
