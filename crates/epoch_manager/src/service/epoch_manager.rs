//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{
    cmp,
    collections::HashMap,
    mem,
    num::NonZeroU32,
    sync::{atomic, atomic::AtomicU64, Arc},
};

use log::*;
use tari_common_types::types::FixedHash;
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_engine_types::ConvertFromByteType;
use tari_ootle_common_types::{
    committee::{Committee, CommitteeInfo, CommitteeMember},
    displayable::Displayable,
    layer_one_transaction::{LayerOnePayloadType, LayerOneTransactionDef},
    optional::Optional,
    DerivableFromPublicKey,
    Epoch,
    NodeAddressable,
    ShardGroup,
    SubstateAddress,
    VotePower,
};
use tari_ootle_storage::global::{models::ValidatorNode, DbEpoch, GlobalDb, MetadataKey};
use tari_ootle_storage_sqlite::global::SqliteGlobalDbAdapter;
use tari_sidechain::EvictionProof;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
use tokio::sync::{broadcast, oneshot};

use crate::{
    error::EpochManagerError,
    service::{config::EpochManagerConfig, NetworkDescription, ShardGroupInfo},
    traits::{EpochManagerSpec, LayerOneTransactionSubmitter},
    EpochManagerEvent,
};

const LOG_TARGET: &str = "tari::ootle::epoch_manager::base_layer";

pub struct EpochManager<TSpec: EpochManagerSpec> {
    global_db: GlobalDb<SqliteGlobalDbAdapter<TSpec::Addr>>,
    config: EpochManagerConfig,
    has_epoch_changed: bool,
    current_epoch_hash: FixedHash,
    tx_events: broadcast::Sender<EpochManagerEvent>,
    waiting_for_scanning_complete: Vec<oneshot::Sender<Result<(), EpochManagerError>>>,
    node_public_key: RistrettoPublicKeyBytes,
    current_shard_key: Option<SubstateAddress>,
    layer_one_submitter: TSpec::LayerOneSubmitter,
    is_initial_epoch_sync_complete: bool,
    current_epoch: Arc<AtomicU64>,
}

impl<TSpec> EpochManager<TSpec>
where TSpec: EpochManagerSpec
{
    pub fn new(
        config: EpochManagerConfig,
        global_db: GlobalDb<SqliteGlobalDbAdapter<TSpec::Addr>>,
        layer_one_submitter: TSpec::LayerOneSubmitter,
        tx_events: broadcast::Sender<EpochManagerEvent>,
        node_public_key: RistrettoPublicKeyBytes,
        current_epoch_atomic: Arc<AtomicU64>,
    ) -> Self {
        Self {
            global_db,
            config,
            current_epoch_hash: FixedHash::zero(),
            has_epoch_changed: false,
            waiting_for_scanning_complete: Vec::new(),
            tx_events,
            node_public_key,
            current_shard_key: None,
            layer_one_submitter,
            is_initial_epoch_sync_complete: false,
            current_epoch: current_epoch_atomic,
        }
    }

    pub async fn load_initial_state(&mut self) -> Result<(), EpochManagerError> {
        info!(target: LOG_TARGET, "Retrieving current epoch and block info from database");
        let mut tx = self.global_db.create_transaction()?;
        let mut metadata = self.global_db.metadata(&mut tx);
        let current_epoch = metadata
            .get_metadata(MetadataKey::EpochManagerCurrentEpoch.as_key_bytes())?
            .unwrap_or(Epoch(0));
        self.set_current_epoch(current_epoch);
        self.current_shard_key = metadata.get_metadata(MetadataKey::EpochManagerCurrentShardKey.as_key_bytes())?;
        self.current_epoch_hash = metadata
            .get_metadata(MetadataKey::EpochManagerLastEpochHash.as_key_bytes())?
            .unwrap_or(Default::default());
        Ok(())
    }

    pub async fn activate_epoch(&mut self, epoch: Epoch, epoch_hash: FixedHash) -> Result<(), EpochManagerError> {
        if self.current_epoch() >= epoch {
            // no need to update the epoch
            return Ok(());
        }

        self.has_epoch_changed = true;

        // In the base layer case, the epoch_hash is the first block of the epoch
        // persist the epoch data including the validator node set
        self.insert_current_epoch(epoch, epoch_hash)?;
        self.assign_validators_for_epoch(epoch)?;
        Ok(())
    }

    /// Assigns validators for the given epoch (makes them active) from the database.
    /// Max number of validators must be passed to limit the number of validators to make active in the given epoch.
    fn assign_validators_for_epoch(&mut self, epoch: Epoch) -> Result<(), EpochManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        let mut validator_nodes = self.global_db.validator_nodes(&mut tx);

        let vns = validator_nodes.get_all_registered_within_start_epoch(epoch)?;

        let num_committees = calculate_num_committees(vns.len() as u64, self.config.committee_size);
        for vn in vns {
            validator_nodes.set_committee_shard(
                vn.shard_key,
                vn.shard_key.to_shard_group(self.config.num_preshards, num_committees),
                epoch,
            )?;
        }

        tx.commit()?;

        Ok(())
    }

    pub fn add_validator_node_registration(
        &mut self,
        activation_epoch: Epoch,
        validator_public_key: RistrettoPublicKeyBytes,
        claim_public_key: RistrettoPublicKeyBytes,
        shard_key: SubstateAddress,
        power: VotePower,
    ) -> Result<(), EpochManagerError> {
        info!(target: LOG_TARGET, "Registering validator node for epoch {}", activation_epoch);

        let Ok(vn_pk) = RistrettoPublicKey::convert_from_byte_type(&validator_public_key) else {
            return Err(EpochManagerError::InvalidPublicKeyBytes {
                public_key: validator_public_key,
            });
        };

        let mut tx = self.global_db.create_transaction()?;
        self.global_db.validator_nodes(&mut tx).insert_validator_node(
            TSpec::Addr::derive_from_public_key(&vn_pk),
            validator_public_key,
            shard_key,
            activation_epoch,
            claim_public_key,
            power,
        )?;

        if validator_public_key == self.node_public_key {
            let mut metadata = self.global_db.metadata(&mut tx);
            metadata.set_metadata(MetadataKey::EpochManagerCurrentShardKey.as_key_bytes(), &shard_key)?;
            metadata.set_metadata(
                MetadataKey::EpochManagerFeeClaimPublicKey.as_key_bytes(),
                &claim_public_key,
            )?;
            self.current_shard_key = Some(shard_key);
            info!(
                target: LOG_TARGET,
                "📋️ This validator node is registered for epoch {}, shard key: {} ", activation_epoch, shard_key
            );
        }

        tx.commit()?;

        Ok(())
    }

    pub async fn deactivate_validator_node(
        &mut self,
        public_key: RistrettoPublicKeyBytes,
        deactivation_epoch: Epoch,
    ) -> Result<(), EpochManagerError> {
        info!(target: LOG_TARGET, "Deactivating validator node({}) registration", public_key);

        let mut tx = self.global_db.create_transaction()?;
        self.global_db
            .validator_nodes(&mut tx)
            .deactivate(public_key, deactivation_epoch)?;
        tx.commit()?;

        Ok(())
    }

    fn insert_current_epoch(&mut self, epoch: Epoch, epoch_hash: FixedHash) -> Result<(), EpochManagerError> {
        let epoch_height = epoch.0;
        let db_epoch = DbEpoch {
            epoch: epoch_height,
            // TODO: remove validator node mr from epoch db
            validator_node_mr: FixedHash::default().as_slice().to_vec(),
        };

        let mut tx = self.global_db.create_transaction()?;

        self.global_db.epochs(&mut tx).insert_epoch(db_epoch)?;
        let mut metadata = self.global_db.metadata(&mut tx);
        metadata.set_metadata(MetadataKey::EpochManagerCurrentEpoch.as_key_bytes(), &epoch)?;
        metadata.set_metadata(MetadataKey::EpochManagerLastEpochHash.as_key_bytes(), &epoch_hash)?;

        tx.commit()?;
        self.set_current_epoch(epoch);
        self.current_epoch_hash = epoch_hash;
        Ok(())
    }

    pub fn current_epoch(&self) -> Epoch {
        self.current_epoch.load(atomic::Ordering::SeqCst).into()
    }

    fn set_current_epoch(&self, epoch: Epoch) {
        self.current_epoch.store(epoch.as_u64(), atomic::Ordering::SeqCst);
    }

    pub fn current_epoch_hash(&self) -> FixedHash {
        self.current_epoch_hash
    }

    pub fn get_validator_node_by_public_key(
        &self,
        epoch: Epoch,
        public_key: &RistrettoPublicKeyBytes,
    ) -> Result<Option<ValidatorNode<TSpec::Addr>>, EpochManagerError> {
        trace!(
            target: LOG_TARGET,
            "get_validator_node: epoch {} with public key {}", epoch, public_key,
        );
        let mut tx = self.global_db.create_transaction()?;
        let vn = self
            .global_db
            .validator_nodes(&mut tx)
            .get_by_public_key(epoch, public_key)
            .optional()?;

        Ok(vn)
    }

    fn get_validator_node_by_address(
        &self,
        epoch: Epoch,
        address: &TSpec::Addr,
    ) -> Result<Option<ValidatorNode<TSpec::Addr>>, EpochManagerError> {
        trace!(
            target: LOG_TARGET,
            "get_validator_node: epoch {} with public key {}", epoch, address,
        );
        let mut tx = self.global_db.create_transaction()?;
        let vn = self
            .global_db
            .validator_nodes(&mut tx)
            .get_by_address(epoch, address)
            .optional()?;

        Ok(vn)
    }

    pub fn get_committees(
        &self,
        epoch: Epoch,
    ) -> Result<HashMap<ShardGroup, Committee<TSpec::Addr>>, EpochManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        let mut validator_node_db = self.global_db.validator_nodes(&mut tx);
        Ok(validator_node_db.get_committees(epoch)?)
    }

    pub fn get_committee_info_by_validator_address(
        &self,
        epoch: Epoch,
        validator_addr: TSpec::Addr,
    ) -> Result<CommitteeInfo, EpochManagerError> {
        let vn = self
            .get_validator_node_by_address(epoch, &validator_addr)?
            .ok_or_else(|| EpochManagerError::ValidatorNodeNotRegistered {
                address: validator_addr.to_string(),
                epoch,
            })?;
        self.get_committee_info_for_substate(epoch, vn.shard_key)
    }

    pub(crate) fn get_committee_vns_from_shard_key(
        &self,
        epoch: Epoch,
        substate_address: SubstateAddress,
    ) -> Result<Vec<ValidatorNode<TSpec::Addr>>, EpochManagerError> {
        let num_vns = self.get_total_validator_count(epoch)?;
        if num_vns == 0 {
            return Err(EpochManagerError::NoCommitteeVns {
                substate_address,
                epoch,
            });
        }

        let num_committees = calculate_num_committees(num_vns, self.config.committee_size);
        if num_committees == 1 {
            // retrieve the validator nodes for this epoch from database, sorted by shard_key
            return self.get_validator_nodes_per_epoch(epoch);
        }

        let shard_group = substate_address.to_shard_group(self.config.num_preshards, num_committees);

        // TODO(perf): fetch full validator node records for the shard group in single query (current O(n + 1) queries)
        let committees = self.get_committee_for_shard_group(epoch, shard_group, false, None)?;

        let mut res = vec![];
        for member in committees {
            let vn = self
                .get_validator_node_by_public_key(epoch, &member.public_key)?
                .ok_or_else(|| EpochManagerError::ValidatorNodeNotRegistered {
                    address: RistrettoPublicKey::convert_from_byte_type(&member.public_key)
                        .ok()
                        .and_then(|pk| TSpec::Addr::try_from_public_key(&pk))
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| format!("PARSE FAIL for pk bytes {}", member.public_key)),
                    epoch,
                })?;
            res.push(vn);
        }
        res.sort_by(|a, b| a.shard_key.cmp(&b.shard_key));
        Ok(res)
    }

    pub(crate) fn get_committee_for_substate(
        &self,
        epoch: Epoch,
        substate_address: SubstateAddress,
    ) -> Result<Committee<TSpec::Addr>, EpochManagerError> {
        let result = self.get_committee_vns_from_shard_key(epoch, substate_address)?;
        Ok(Committee::new(
            result
                .into_iter()
                .map(|v| CommitteeMember {
                    public_key: v.public_key,
                    address: v.address,
                    vote_power: v.vote_power,
                })
                .collect(),
        ))
    }

    pub fn get_number_of_committees(&self, epoch: Epoch) -> Result<u32, EpochManagerError> {
        let num_vns = self.get_total_validator_count(epoch)?;
        Ok(calculate_num_committees(num_vns, self.config.committee_size))
    }

    pub fn get_validator_nodes_per_epoch(
        &self,
        epoch: Epoch,
    ) -> Result<Vec<ValidatorNode<TSpec::Addr>>, EpochManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        let vns = self.global_db.validator_nodes(&mut tx).get_all_within_epoch(epoch)?;
        Ok(vns)
    }

    pub fn is_initial_epoch_sync_complete(&self) -> bool {
        self.is_initial_epoch_sync_complete
    }

    pub async fn on_scanning_complete(&mut self) -> Result<(), EpochManagerError> {
        if !self.is_initial_epoch_sync_complete {
            info!(
                target: LOG_TARGET,
                "🌟 Initial epoch sync complete. Current epoch is {}", self.current_epoch()
            );
            self.is_initial_epoch_sync_complete = true;
            for reply in mem::take(&mut self.waiting_for_scanning_complete) {
                let _ignore = reply.send(Ok(()));
            }
        }

        if self.has_epoch_changed {
            let num_committees = self.get_number_of_committees(self.current_epoch())?;
            let epoch = self.current_epoch();
            let shard_group = self
                .get_our_validator_node(epoch)
                .optional()?
                .map(|vn| vn.shard_key.to_shard_group(self.config.num_preshards, num_committees));
            let level = if self.is_initial_epoch_sync_complete {
                Level::Info
            } else {
                Level::Debug
            };
            log!(target: LOG_TARGET, level, "🌟 A new epoch {} is upon us. Shard group: {}", epoch, shard_group.display());

            self.publish_event(EpochManagerEvent::EpochChanged {
                epoch: self.current_epoch(),
                registered_shard_group: shard_group,
            });
            self.has_epoch_changed = false;
        }

        Ok(())
    }

    pub fn add_notify_on_scanning_complete(&mut self, reply: oneshot::Sender<Result<(), EpochManagerError>>) {
        if self.is_initial_epoch_sync_complete {
            let _ignore = reply.send(Ok(()));
        } else {
            self.waiting_for_scanning_complete.push(reply);
        }
    }

    pub fn get_our_validator_node(&self, epoch: Epoch) -> Result<ValidatorNode<TSpec::Addr>, EpochManagerError> {
        let vn = self
            .get_validator_node_by_public_key(epoch, &self.node_public_key)?
            .ok_or_else(|| EpochManagerError::ValidatorNodeNotRegistered {
                address: RistrettoPublicKey::convert_from_byte_type(&self.node_public_key)
                    .ok()
                    .and_then(|pk| TSpec::Addr::try_from_public_key(&pk))
                    .map(|a| a.to_string())
                    .unwrap_or_else(|| format!("PARSE FAIL for pk bytes {}", self.node_public_key)),
                epoch,
            })?;
        Ok(vn)
    }

    pub fn get_total_validator_count(&self, epoch: Epoch) -> Result<u64, EpochManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        let count = self.global_db.validator_nodes(&mut tx).count(epoch)?;
        Ok(count)
    }

    pub fn get_num_committees(&self, epoch: Epoch) -> Result<u32, EpochManagerError> {
        let total_vns = self.get_total_validator_count(epoch)?;
        let committee_size = self.config.committee_size;
        let num_committees = calculate_num_committees(total_vns, committee_size);
        Ok(num_committees)
    }

    pub fn get_committee_info_for_substate(
        &self,
        epoch: Epoch,
        substate_address: SubstateAddress,
    ) -> Result<CommitteeInfo, EpochManagerError> {
        let num_committees = self.get_number_of_committees(epoch)?;
        let shard_group = substate_address.to_shard_group(self.config.num_preshards, num_committees);
        self.get_committee_info(epoch, shard_group)
    }

    pub fn get_committee_info(
        &self,
        epoch: Epoch,
        shard_group: ShardGroup,
    ) -> Result<CommitteeInfo, EpochManagerError> {
        let num_committees = self.get_number_of_committees(epoch)?;
        let mut tx = self.global_db.create_transaction()?;
        let mut validator_node_db = self.global_db.validator_nodes(&mut tx);
        let num_validators = validator_node_db.count_in_shard_group(epoch, shard_group)?;
        // NOTE: currently each validator has a vote power of 1, so the total vote power is equal to the number of
        // validators. This may change in the future if e.g. we introduce staking.
        let total_vote_power = VotePower::of(num_validators);
        let num_validators = u32::try_from(num_validators).map_err(|_| EpochManagerError::IntegerOverflow {
            func: "get_committee_info",
        })?;
        Ok(CommitteeInfo::new(
            self.config.num_preshards,
            num_validators,
            num_committees,
            shard_group,
            epoch,
            total_vote_power,
        ))
    }

    pub fn get_local_committee_info(&self, epoch: Epoch) -> Result<CommitteeInfo, EpochManagerError> {
        let vn = self
            .get_validator_node_by_public_key(epoch, &self.node_public_key)?
            .ok_or_else(|| EpochManagerError::ValidatorNodeNotRegistered {
                address: self.node_public_key.to_string(),
                epoch,
            })?;
        self.get_committee_info_for_substate(epoch, vn.shard_key)
    }

    pub(crate) fn get_committee_for_shard_group(
        &self,
        epoch: Epoch,
        shard_group: ShardGroup,
        shuffle: bool,
        limit: Option<usize>,
    ) -> Result<Committee<TSpec::Addr>, EpochManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        let mut validator_node_db = self.global_db.validator_nodes(&mut tx);
        let committees = validator_node_db.get_committee_for_shard_group(
            epoch,
            shard_group,
            shuffle,
            limit.unwrap_or(usize::MAX),
        )?;
        Ok(committees)
    }

    pub(crate) fn get_committees_overlapping_shard_group(
        &self,
        epoch: Epoch,
        shard_group: ShardGroup,
    ) -> Result<HashMap<ShardGroup, Committee<TSpec::Addr>>, EpochManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        let mut validator_node_db = self.global_db.validator_nodes(&mut tx);
        let committees = validator_node_db.get_committees_overlapping_shard_group(epoch, shard_group)?;
        Ok(committees)
    }

    pub(crate) fn get_random_committee_member_from_shard_group(
        &self,
        epoch: Epoch,
        shard_group: Option<ShardGroup>,
        excluding: Vec<TSpec::Addr>,
    ) -> Result<ValidatorNode<TSpec::Addr>, EpochManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        let mut validator_node_db = self.global_db.validator_nodes(&mut tx);
        let vn = validator_node_db.get_random_committee_member_from_shard_group(epoch, shard_group, excluding)?;
        Ok(vn)
    }

    pub fn get_fee_claim_public_key(&self) -> Result<Option<RistrettoPublicKeyBytes>, EpochManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        let mut metadata = self.global_db.metadata(&mut tx);
        let fee_claim_public_key = metadata.get_metadata(MetadataKey::EpochManagerFeeClaimPublicKey.as_key_bytes())?;
        Ok(fee_claim_public_key)
    }

    fn publish_event(&mut self, event: EpochManagerEvent) {
        let _ignore = self.tx_events.send(event);
    }

    pub async fn add_intent_to_evict_validator(&self, proof: EvictionProof) -> Result<(), EpochManagerError> {
        {
            let mut tx = self.global_db.create_transaction()?;
            // Currently we store this for ease of debugging, there is no specific need to store this in the database
            let mut bl = self.global_db.base_layer(&mut tx);
            bl.insert_eviction_proof(&proof)?;
            tx.commit()?;
        }

        let proof = LayerOneTransactionDef {
            payload_type: LayerOnePayloadType::EvictionProof,
            payload: proof,
        };

        self.layer_one_submitter
            .submit_transaction(proof)
            .await
            .map_err(|e| EpochManagerError::FailedToSubmitLayerOneTransaction { details: e.to_string() })?;

        Ok(())
    }

    pub fn get_network_description(&self) -> Result<NetworkDescription, EpochManagerError> {
        let epoch = self.current_epoch();
        let num_committees = self.get_number_of_committees(epoch)?;
        let shard_groups = self.config.num_preshards.all_shard_groups_iter(num_committees);

        let mut tx = self.global_db.create_transaction()?;
        let mut validator_node_db = self.global_db.validator_nodes(&mut tx);

        let shard_groups = shard_groups
            .map(|shard_group| {
                let num_members = validator_node_db.count_in_shard_group(epoch, shard_group)?;
                let num_members = u32::try_from(num_members).map_err(|_| EpochManagerError::IntegerOverflow {
                    func: "get_network_description",
                })?;
                Ok((shard_group, ShardGroupInfo { num_members }))
            })
            .collect::<Result<_, EpochManagerError>>()?;

        Ok(NetworkDescription {
            epoch,
            shard_groups,
            num_preshards: self.config.num_preshards,
        })
    }
}

fn calculate_num_committees(num_vns: u64, committee_size: NonZeroU32) -> u32 {
    // Number of committees is proportional to the number of validators available.
    // We cap the number of committees to u32::MAX (for a committee_size of 10 that's over 42 billion validators)
    cmp::min(
        cmp::max(1, num_vns / u64::from(committee_size.get())),
        u64::from(u32::MAX),
    ) as u32
}
