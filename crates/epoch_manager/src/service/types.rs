//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use tari_common_types::types::FixedHash;
use tari_ootle_common_types::{
    committee::{Committee, CommitteeInfo},
    Epoch,
    ShardGroup,
    SubstateAddress,
};
use tari_ootle_storage::global::models::ValidatorNode;
use tari_sidechain::EvictionProof;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
use tokio::sync::oneshot;

use crate::error::EpochManagerError;

type Reply<T> = oneshot::Sender<Result<T, EpochManagerError>>;

#[derive(Debug)]
pub enum EpochManagerRequest<TAddr> {
    CurrentEpoch {
        reply: Reply<Epoch>,
    },
    CurrentEpochHash {
        reply: Reply<FixedHash>,
    },
    GetValidatorNodeByPublicKey {
        epoch: Epoch,
        public_key: RistrettoPublicKeyBytes,
        reply: Reply<ValidatorNode<TAddr>>,
    },
    AddValidatorNodeRegistration {
        activation_epoch: Epoch,
        validator_public_key: RistrettoPublicKeyBytes,
        claim_public_key: RistrettoPublicKeyBytes,
        value_of_registration: u64,
        shard_key: SubstateAddress,
        reply: Reply<()>,
    },
    DeactivateValidatorNode {
        public_key: RistrettoPublicKeyBytes,
        deactivation_epoch: Epoch,
        reply: Reply<()>,
    },
    ActivateEpoch {
        epoch: Epoch,
        epoch_hash: FixedHash,
        reply: Reply<()>,
    },
    GetCommittees {
        epoch: Epoch,
        reply: Reply<HashMap<ShardGroup, Committee<TAddr>>>,
    },
    GetCommitteeForSubstate {
        epoch: Epoch,
        substate_address: SubstateAddress,
        reply: Reply<Committee<TAddr>>,
    },
    GetCommitteeInfoByAddress {
        epoch: Epoch,
        address: TAddr,
        reply: Reply<CommitteeInfo>,
    },
    GetValidatorNodesPerEpoch {
        epoch: Epoch,
        reply: Reply<Vec<ValidatorNode<TAddr>>>,
    },
    WaitForInitialScanningToComplete {
        reply: Reply<()>,
    },
    IsInitialScanningComplete {
        reply: Reply<bool>,
    },
    GetOurValidatorNode {
        epoch: Epoch,
        reply: Reply<ValidatorNode<TAddr>>,
    },
    GetCommitteeInfo {
        epoch: Epoch,
        substate_address: SubstateAddress,
        reply: Reply<CommitteeInfo>,
    },
    GetLocalCommitteeInfo {
        epoch: Epoch,
        reply: Reply<CommitteeInfo>,
    },
    GetNumCommittees {
        epoch: Epoch,
        reply: Reply<u32>,
    },
    GetCommitteeForShardGroup {
        epoch: Epoch,
        shard_group: ShardGroup,
        limit: Option<usize>,
        reply: Reply<Committee<TAddr>>,
    },
    GetCommitteesOverlappingShardGroup {
        epoch: Epoch,
        shard_group: ShardGroup,
        reply: Reply<HashMap<ShardGroup, Committee<TAddr>>>,
    },
    GetFeeClaimPublicKey {
        reply: Reply<Option<RistrettoPublicKeyBytes>>,
    },
    AddIntentToEvictValidator {
        proof: Box<EvictionProof>,
        reply: Reply<()>,
    },
    GetRandomCommitteeMemberFromShardGroup {
        epoch: Epoch,
        shard_group: Option<ShardGroup>,
        excluding: Vec<TAddr>,
        reply: Reply<ValidatorNode<TAddr>>,
    },
}
