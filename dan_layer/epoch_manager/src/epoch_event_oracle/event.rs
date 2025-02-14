//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::{FixedHash, PublicKey};
use tari_dan_common_types::{Epoch, SubstateAddress};
use tari_engine_types::{confidential::UnclaimedConfidentialOutput, TemplateAddress};
use tari_sidechain::EvictionProof;
use url::Url;

#[derive(Debug)]
pub enum EpochEvent {
    Error(anyhow::Error),
    ActiveValidatorNodeSetChanged {
        epoch: Epoch,
        node_changes: Vec<ValidatorNodeChange>,
    },
    NewValidatorRegistered {
        epoch: Epoch,
        claim_public_key: PublicKey,
        validator_node_public_key: PublicKey,
    },
    NewCodeTemplateDownload {
        epoch: Epoch,
        name: String,
        address: TemplateAddress,
        author_public_key: PublicKey,
        url: Url,
        binary_hash: FixedHash,
    },
    NewConfidentialOutput {
        epoch: Epoch,
        substate: UnclaimedConfidentialOutput,
    },
    NewEvictionProof {
        epoch: Epoch,
        eviction_proof: EvictionProof,
    },
    EpochChanged {
        epoch: Epoch,
        epoch_hash: FixedHash,
    },
    DoneForNow {
        epoch: Epoch,
    },
}

impl EpochEvent {
    pub fn error<E: Into<anyhow::Error>>(e: E) -> Self {
        EpochEvent::Error(e.into())
    }
}

/// Represents a validator node state change
#[derive(Debug, Clone)]
pub enum ValidatorNodeChange {
    Add {
        claim_public_key: PublicKey,
        validator_node_public_key: PublicKey,
        activation_epoch: Epoch,
        minimum_value_promise: u64,
        shard_key: SubstateAddress,
    },
    Remove {
        public_key: PublicKey,
    },
}
