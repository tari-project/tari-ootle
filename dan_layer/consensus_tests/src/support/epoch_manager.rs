//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use tari_common_types::types::{FixedHash, PublicKey};
use tari_dan_common_types::{
    committee::{Committee, CommitteeInfo},
    Epoch,
    ShardGroup,
    SubstateAddress,
    ToSubstateAddress,
    VersionedSubstateId,
};
use tari_dan_storage::global::models::ValidatorNode;
use tari_epoch_manager::{EpochManagerError, EpochManagerEvent, EpochManagerReader};
use tokio::sync::{broadcast, Mutex, MutexGuard};

use crate::support::{address::TestAddress, helpers::random_substate_in_shard_group, TEST_NUM_PRESHARDS};

#[derive(Debug, Clone)]
pub struct TestEpochManager {
    inner: Arc<Mutex<TestEpochManagerState>>,
    our_validator_node: Option<ValidatorNode<TestAddress>>,
    tx_epoch_events: broadcast::Sender<EpochManagerEvent>,
    current_epoch: Epoch,
}

impl TestEpochManager {
    pub fn new(tx_epoch_events: broadcast::Sender<EpochManagerEvent>) -> Self {
        Self {
            inner: Default::default(),
            our_validator_node: None,
            tx_epoch_events,
            current_epoch: Epoch(0),
        }
    }

    pub async fn set_current_epoch(&mut self, current_epoch: Epoch, shard_group: ShardGroup) -> &Self {
        self.current_epoch = current_epoch;
        {
            let mut lock = self.inner.lock().await;
            lock.current_epoch = current_epoch;
            lock.is_epoch_active = true;
        }

        let _ = self.tx_epoch_events.send(EpochManagerEvent::EpochChanged {
            epoch: current_epoch,
            registered_shard_group: Some(shard_group),
        });

        self
    }

    pub async fn state_lock(&self) -> MutexGuard<TestEpochManagerState> {
        self.inner.lock().await
    }

    pub fn clone_for(&self, address: TestAddress, public_key: PublicKey, shard_key: SubstateAddress) -> Self {
        let mut copy = self.clone();
        if let Some(our_validator_node) = self.our_validator_node.clone() {
            copy.our_validator_node = Some(ValidatorNode {
                address,
                public_key: public_key.clone(),
                shard_key,
                start_epoch: our_validator_node.start_epoch,
                end_epoch: None,
                fee_claim_public_key: public_key,
            });
        } else {
            copy.our_validator_node = Some(ValidatorNode {
                address,
                public_key: public_key.clone(),
                shard_key,
                start_epoch: Epoch(0),
                end_epoch: None,
                fee_claim_public_key: public_key,
            });
        }
        copy
    }

    pub async fn add_committees(&self, committees: HashMap<ShardGroup, Committee<TestAddress>>) {
        let mut state = self.state_lock().await;
        for (shard_group, committee) in committees {
            for (address, pk) in &committee.members {
                let substate_id = random_substate_in_shard_group(shard_group, TEST_NUM_PRESHARDS);
                let substate_id = VersionedSubstateId::new(substate_id, 0);
                state.validator_nodes.insert(
                    address.clone(),
                    (
                        ValidatorNode {
                            address: address.clone(),
                            public_key: pk.clone(),
                            shard_key: substate_id.to_substate_address(),
                            start_epoch: Epoch(0),
                            end_epoch: None,
                            fee_claim_public_key: pk.clone(),
                        },
                        shard_group,
                    ),
                );
                state.address_shard.insert(address.clone(), shard_group);
            }

            state.committees.insert(shard_group, committee);
        }
    }

    pub async fn all_validators(&self) -> Vec<(ValidatorNode<TestAddress>, ShardGroup)> {
        self.state_lock().await.validator_nodes.values().cloned().collect()
    }

    pub async fn all_committees(&self) -> HashMap<ShardGroup, Committee<TestAddress>> {
        self.state_lock().await.committees.clone()
    }

    pub fn get_current_epoch(&self) -> Epoch {
        self.current_epoch
    }

    pub async fn eviction_proofs(&self) -> Vec<tari_sidechain::EvictionProof> {
        self.state_lock().await.eviction_proofs.clone()
    }
}

#[async_trait]
impl EpochManagerReader for TestEpochManager {
    type Addr = TestAddress;

    fn subscribe(&self) -> broadcast::Receiver<EpochManagerEvent> {
        self.tx_epoch_events.subscribe()
    }

    async fn get_committee_for_substate(
        &self,
        _epoch: Epoch,
        substate_address: SubstateAddress,
    ) -> Result<Committee<Self::Addr>, EpochManagerError> {
        let state = self.state_lock().await;
        let shard_group = substate_address.to_shard_group(TEST_NUM_PRESHARDS, state.committees.len() as u32);
        Ok(state.committees[&shard_group].clone())
    }

    async fn get_our_validator_node(&self, _epoch: Epoch) -> Result<ValidatorNode<TestAddress>, EpochManagerError> {
        Ok(self.our_validator_node.clone().unwrap())
    }

    async fn get_validator_node(
        &self,
        _epoch: Epoch,
        addr: &Self::Addr,
    ) -> Result<ValidatorNode<Self::Addr>, EpochManagerError> {
        let (vn, _) = self.state_lock().await.validator_nodes[addr].clone();
        Ok(vn)
    }

    async fn get_all_validator_nodes(
        &self,
        _epoch: Epoch,
    ) -> Result<Vec<ValidatorNode<Self::Addr>>, EpochManagerError> {
        todo!()
    }

    async fn get_local_committee_info(&self, epoch: Epoch) -> Result<CommitteeInfo, EpochManagerError> {
        let our_vn = self.get_our_validator_node(epoch).await?;
        let num_committees = self.get_num_committees(epoch).await?;
        let sg = our_vn.shard_key.to_shard_group(TEST_NUM_PRESHARDS, num_committees);
        let num_shard_group_members = self
            .inner
            .lock()
            .await
            .committees
            .get(&sg)
            .map(|c| c.len())
            .unwrap_or(0);

        Ok(CommitteeInfo::new(
            TEST_NUM_PRESHARDS,
            num_shard_group_members as u32,
            num_committees,
            sg,
            epoch,
        ))
    }

    async fn current_epoch(&self) -> Result<Epoch, EpochManagerError> {
        Ok(self.inner.lock().await.current_epoch)
    }

    async fn current_base_layer_block_info(&self) -> Result<(u64, FixedHash), EpochManagerError> {
        Ok(self.inner.lock().await.current_block_info)
    }

    async fn get_last_block_of_current_epoch(&self) -> Result<FixedHash, EpochManagerError> {
        Ok(self.inner.lock().await.last_block_of_current_epoch)
    }

    async fn is_last_block_of_epoch(&self, _block_height: u64) -> Result<bool, EpochManagerError> {
        Ok(false)
    }

    async fn is_epoch_active(&self, _epoch: Epoch) -> Result<bool, EpochManagerError> {
        Ok(self.inner.lock().await.is_epoch_active)
    }

    async fn get_num_committees(&self, _epoch: Epoch) -> Result<u32, EpochManagerError> {
        Ok(self.inner.lock().await.committees.len() as u32)
    }

    async fn get_committees(
        &self,
        _epoch: Epoch,
    ) -> Result<HashMap<ShardGroup, Committee<Self::Addr>>, EpochManagerError> {
        Ok(self.inner.lock().await.committees.clone())
    }

    async fn get_committee_info_by_validator_address(
        &self,
        epoch: Epoch,
        address: &Self::Addr,
    ) -> Result<CommitteeInfo, EpochManagerError> {
        let state = self.state_lock().await;
        let (sg, committee) = state
            .committees
            .iter()
            .find(|(_, committee)| committee.iter().any(|(addr, _)| addr == address))
            .unwrap_or_else(|| panic!("Validator {address} not found in any committee"));
        let num_committees = state.committees.len() as u32;
        let num_members = committee.len();
        Ok(CommitteeInfo::new(
            TEST_NUM_PRESHARDS,
            num_members as u32,
            num_committees,
            *sg,
            epoch,
        ))
    }

    async fn get_committee_by_shard_group(
        &self,
        _epoch: Epoch,
        shard_group: ShardGroup,
        limit: Option<usize>,
    ) -> Result<Committee<Self::Addr>, EpochManagerError> {
        let state = self.state_lock().await;
        let Some(mut committee) = state.committees.get(&shard_group).cloned() else {
            panic!("Committee not found for shard group {}", shard_group);
        };

        if let Some(limit) = limit {
            committee.members.truncate(limit);
        }

        Ok(committee)
    }

    async fn get_committees_overlapping_shard_group(
        &self,
        _epoch: Epoch,
        shard_group: ShardGroup,
    ) -> Result<HashMap<ShardGroup, Committee<Self::Addr>>, EpochManagerError> {
        let state = self.state_lock().await;
        let mut committees = HashMap::new();
        for (sg, committee) in &state.committees {
            if sg.overlaps_shard_group(&shard_group) {
                committees.insert(*sg, committee.clone());
            }
        }
        Ok(committees)
    }

    async fn get_committee_info_for_substate(
        &self,
        epoch: Epoch,
        substate_address: SubstateAddress,
    ) -> Result<CommitteeInfo, EpochManagerError> {
        let num_committees = self.get_num_committees(epoch).await?;
        let sg = substate_address.to_shard_group(TEST_NUM_PRESHARDS, num_committees);
        let num_members = self
            .inner
            .lock()
            .await
            .committees
            .get(&sg)
            .map(|c| c.len())
            .unwrap_or(0);

        Ok(CommitteeInfo::new(
            TEST_NUM_PRESHARDS,
            num_members as u32,
            num_committees,
            sg,
            epoch,
        ))
    }

    async fn get_validator_node_by_public_key(
        &self,
        _epoch: Epoch,
        public_key: PublicKey,
    ) -> Result<ValidatorNode<Self::Addr>, EpochManagerError> {
        let lock = self.state_lock().await;
        let (vn, _) = lock
            .validator_nodes
            .values()
            .find(|(vn, _)| vn.public_key == public_key)
            .unwrap_or_else(|| panic!("Validator node not found for public key {}", public_key));

        Ok(ValidatorNode {
            address: vn.address.clone(),
            public_key: vn.public_key.clone(),
            shard_key: vn.shard_key,
            start_epoch: vn.start_epoch,
            end_epoch: vn.end_epoch,
            fee_claim_public_key: vn.fee_claim_public_key.clone(),
        })
    }

    async fn get_base_layer_block_height(&self, _hash: FixedHash) -> Result<Option<u64>, EpochManagerError> {
        Ok(Some(self.inner.lock().await.current_block_info.0))
    }

    async fn wait_for_initial_scanning_to_complete(&self) -> Result<(), EpochManagerError> {
        // Scanning is not relevant to tests
        Ok(())
    }

    async fn add_intent_to_evict_validator(
        &self,
        proof: tari_sidechain::EvictionProof,
    ) -> Result<(), EpochManagerError> {
        let mut state = self.state_lock().await;
        state.eviction_proofs.push(proof);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct TestEpochManagerState {
    pub current_epoch: Epoch,
    pub current_block_info: (u64, FixedHash),
    pub last_block_of_current_epoch: FixedHash,
    pub is_epoch_active: bool,
    #[allow(clippy::type_complexity)]
    pub validator_nodes: HashMap<TestAddress, (ValidatorNode<TestAddress>, ShardGroup)>,
    pub committees: HashMap<ShardGroup, Committee<TestAddress>>,
    pub address_shard: HashMap<TestAddress, ShardGroup>,
    pub eviction_proofs: Vec<tari_sidechain::EvictionProof>,
}

impl Default for TestEpochManagerState {
    fn default() -> Self {
        Self {
            current_epoch: Epoch(0),
            current_block_info: (0, FixedHash::default()),
            last_block_of_current_epoch: FixedHash::default(),
            validator_nodes: HashMap::new(),
            is_epoch_active: false,
            committees: HashMap::new(),
            address_shard: HashMap::new(),
            eviction_proofs: Vec::new(),
        }
    }
}
