//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine::traits::ClaimProofVerifier;
use tari_ootle_common_types::Epoch;

pub struct AlwaysPassesProofVerifier;

impl ClaimProofVerifier for AlwaysPassesProofVerifier {
    fn verify_claim_proof(
        &self,
        _epoch: Epoch,
        _claimant: &tari_template_lib::prelude::RistrettoPublicKeyBytes,
        _claim_proof: &tari_engine_types::confidential::MinotariBurnClaimProof,
    ) -> Result<(), String> {
        Ok(())
    }
}
