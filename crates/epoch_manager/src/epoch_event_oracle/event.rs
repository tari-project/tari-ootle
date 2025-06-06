//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use tari_common_types::types::FixedHash;
use tari_engine_types::confidential::UnclaimedConfidentialOutput;
use tari_ootle_common_types::{displayable::Displayable, Epoch, SubstateAddress};
use tari_sidechain::EvictionProof;
use tari_template_lib_types::{crypto::RistrettoPublicKeyBytes, TemplateAddress};
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
        claim_public_key: RistrettoPublicKeyBytes,
        validator_node_public_key: RistrettoPublicKeyBytes,
    },
    NewValidatorNodeExit {
        epoch: Epoch,
        validator_node_public_key: RistrettoPublicKeyBytes,
    },
    NewCodeTemplateDownload {
        epoch: Epoch,
        name: String,
        address: TemplateAddress,
        author_public_key: RistrettoPublicKeyBytes,
        url: Url,
        binary_hash: FixedHash,
    },
    NewConfidentialOutput {
        epoch: Epoch,
        substate: UnclaimedConfidentialOutput,
    },
    NewEvictionProof {
        epoch: Epoch,
        eviction_proof: Box<EvictionProof>,
    },
    EpochChanged {
        epoch: Epoch,
        epoch_hash: FixedHash,
    },
    DoneForNow {
        epoch: Epoch,
        epoch_hash: FixedHash,
    },
}

impl EpochEvent {
    pub fn error<E: Into<anyhow::Error>>(e: E) -> Self {
        EpochEvent::Error(e.into())
    }
}

impl Display for EpochEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EpochEvent::Error(e) => write!(f, "Error: {}", e),
            EpochEvent::ActiveValidatorNodeSetChanged { epoch, node_changes } => {
                write!(
                    f,
                    "ActiveValidatorNodeSetChanged {{ epoch: {}, node_changes: {} }}",
                    epoch,
                    node_changes.display()
                )
            },
            EpochEvent::NewValidatorRegistered {
                epoch,
                claim_public_key,
                validator_node_public_key,
            } => {
                write!(
                    f,
                    "NewValidatorRegistered {{ epoch: {}, claim_public_key: {}, validator_node_public_key: {} }}",
                    epoch, claim_public_key, validator_node_public_key
                )
            },
            EpochEvent::NewValidatorNodeExit {
                epoch,
                validator_node_public_key,
            } => {
                write!(
                    f,
                    "NewValidatorNodeExit {{ epoch: {}, validator_node_public_key: {} }}",
                    epoch, validator_node_public_key
                )
            },
            EpochEvent::NewCodeTemplateDownload {
                epoch,
                name,
                address,
                author_public_key,
                url,
                binary_hash,
            } => {
                write!(
                    f,
                    "NewCodeTemplateDownload {{ epoch: {}, name: {}, address: {}, author_public_key: {}, url: {}, \
                     binary_hash: {} }}",
                    epoch, name, address, author_public_key, url, binary_hash
                )
            },
            EpochEvent::NewConfidentialOutput { epoch, substate } => {
                write!(
                    f,
                    "NewConfidentialOutput {{ epoch: {}, commitment: {} }}",
                    epoch, substate.commitment
                )
            },
            EpochEvent::NewEvictionProof { epoch, eviction_proof } => {
                write!(
                    f,
                    "NewEvictionProof {{ epoch: {}, evict_node: {} }}",
                    epoch,
                    eviction_proof.node_to_evict()
                )
            },
            EpochEvent::EpochChanged { epoch, epoch_hash } => {
                write!(f, "EpochChanged {{ epoch: {}, epoch_hash: {} }}", epoch, epoch_hash)
            },
            EpochEvent::DoneForNow { epoch, epoch_hash } => {
                write!(f, "DoneForNow {{ epoch: {}, hash: {} }}", epoch, epoch_hash)
            },
        }
    }
}

/// Represents a validator node state change
#[derive(Debug, Clone)]
pub enum ValidatorNodeChange {
    Add {
        claim_public_key: RistrettoPublicKeyBytes,
        validator_node_public_key: RistrettoPublicKeyBytes,
        activation_epoch: Epoch,
        minimum_value_promise: u64,
        shard_key: SubstateAddress,
    },
    Remove {
        public_key: RistrettoPublicKeyBytes,
    },
}

impl Display for ValidatorNodeChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidatorNodeChange::Add {
                claim_public_key,
                validator_node_public_key,
                activation_epoch,
                minimum_value_promise,
                shard_key,
            } => write!(
                f,
                "Add {{ claim_public_key: {}, validator_node_public_key: {}, activation_epoch: {}, \
                 minimum_value_promise: {}, shard_key: {} }}",
                claim_public_key, validator_node_public_key, activation_epoch, minimum_value_promise, shard_key
            ),
            ValidatorNodeChange::Remove { public_key } => write!(f, "Remove {{ public_key: {} }}", public_key),
        }
    }
}
