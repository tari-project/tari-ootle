//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use tari_base_node_client::types::BaseLayerConsensusConstants;
use tari_common_types::types::{FixedHash, PublicKey};
use tari_core::transactions::{tari_amount::MicroMinotari, transaction_components::ValidatorNodeRegistration};
use tari_dan_common_types::{
    committee::{Committee, CommitteeInfo},
    Epoch,
    ShardGroup,
    SubstateAddress,
};
use tari_dan_storage::global::models::ValidatorNode;
use tari_sidechain::EvictionProof;
use tokio::sync::oneshot;

use crate::error::EpochManagerError;

type Reply<T> = oneshot::Sender<Result<T, EpochManagerError>>;

#[derive(Debug)]
pub enum EpochManagerRequest<TAddr> {
    CurrentEpoch {
        reply: Reply<Epoch>,
    },
    CurrentBlockInfo {
        reply: Reply<(u64, FixedHash)>,
    },
    GetLastBlockOfTheEpoch {
        reply: Reply<FixedHash>,
    },
    IsLastBlockOfTheEpoch {
        block_height: u64,
        reply: Reply<bool>,
    },
    GetValidatorNode {
        epoch: Epoch,
        addr: TAddr,
        reply: Reply<ValidatorNode<TAddr>>,
    },
    GetValidatorNodeByPublicKey {
        epoch: Epoch,
        public_key: PublicKey,
        reply: Reply<ValidatorNode<TAddr>>,
    },
    GetManyValidatorNodes {
        query: Vec<(Epoch, PublicKey)>,
        reply: Reply<HashMap<(Epoch, PublicKey), ValidatorNode<TAddr>>>,
    },
    AddValidatorNodeRegistration {
        activation_epoch: Epoch,
        registration: ValidatorNodeRegistration,
        value: MicroMinotari,
        reply: Reply<()>,
    },
    DeactivateValidatorNode {
        public_key: PublicKey,
        deactivation_epoch: Epoch,
        reply: Reply<()>,
    },
    AddBlockHash {
        block_height: u64,
        block_hash: FixedHash,
        reply: Reply<()>,
    },
    UpdateEpoch {
        block_height: u64,
        block_hash: FixedHash,
        reply: Reply<()>,
    },
    LastRegistrationEpoch {
        reply: Reply<Option<Epoch>>,
    },
    UpdateLastRegistrationEpoch {
        epoch: Epoch,
        reply: Reply<()>,
    },
    IsEpochValid {
        epoch: Epoch,
        reply: Reply<bool>,
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
    NotifyScanningComplete {
        reply: Reply<()>,
    },
    WaitForInitialScanningToComplete {
        reply: Reply<()>,
    },
    GetBaseLayerConsensusConstants {
        reply: Reply<BaseLayerConsensusConstants>,
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
    GetBaseLayerBlockHeight {
        hash: FixedHash,
        reply: Reply<Option<u64>>,
    },
    GetFeeClaimPublicKey {
        reply: Reply<Option<PublicKey>>,
    },
    SetFeeClaimPublicKey {
        public_key: PublicKey,
        reply: Reply<()>,
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
