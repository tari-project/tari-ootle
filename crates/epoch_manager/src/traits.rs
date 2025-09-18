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

use std::{collections::HashMap, future::Future};

use tari_common_types::types::FixedHash;
use tari_engine_types::confidential::UnclaimedConfidentialOutput;
use tari_ootle_common_types::{
    committee::{Committee, CommitteeInfo},
    layer_one_transaction::LayerOneTransactionDef,
    DerivableFromPublicKey,
    Epoch,
    NodeAddressable,
    ShardGroup,
    SubstateAddress,
    VotePower,
};
use tari_ootle_storage::global::models::ValidatorNode;
use tari_sidechain::EvictionProof;
use tari_template_lib_types::{crypto::RistrettoPublicKeyBytes, TemplateAddress};
use tokio::sync::broadcast;
use url::Url;

use crate::{epoch_event_oracle::EpochEventOracle, EpochManagerError, EpochManagerEvent};

pub trait EpochManagerSpec: Send + 'static {
    type Addr: NodeAddressable + DerivableFromPublicKey + 'static;
    type EpochEventOracle: EpochEventOracle + Send + 'static;
    type UtxoStore: EpochUtxoStore + Send + 'static;
    type LayerOneSubmitter: LayerOneTransactionSubmitter + Send + Sync + 'static;
    type TemplateDownloader: TemplateDownloader + Send + 'static;
}

pub trait EpochManagerWriter: Send + Sync {
    fn add_validator_node_registration(
        &mut self,
        activation_epoch: Epoch,
        validator_public_key: RistrettoPublicKeyBytes,
        claim_public_key: RistrettoPublicKeyBytes,
        shard_key: SubstateAddress,
        power: VotePower,
    ) -> impl Future<Output = Result<(), EpochManagerError>> + Send;

    fn deactivate_validator_node(
        &mut self,
        public_key: RistrettoPublicKeyBytes,
        deactivation_epoch: Epoch,
    ) -> impl Future<Output = Result<(), EpochManagerError>> + Send;

    fn activate_epoch(
        &mut self,
        epoch: Epoch,
        epoch_hash: FixedHash,
    ) -> impl Future<Output = Result<(), EpochManagerError>> + Send;
}

pub trait EpochManagerReader: Send + Sync {
    type Addr: NodeAddressable;

    fn subscribe(&self) -> broadcast::Receiver<EpochManagerEvent>;

    fn wait_for_initial_scanning_to_complete(&self) -> impl Future<Output = Result<(), EpochManagerError>> + Send;

    fn get_all_validator_nodes(
        &self,
        epoch: Epoch,
    ) -> impl Future<Output = Result<Vec<ValidatorNode<Self::Addr>>, EpochManagerError>> + Send;

    fn get_committees(
        &self,
        epoch: Epoch,
    ) -> impl Future<Output = Result<HashMap<ShardGroup, Committee<Self::Addr>>, EpochManagerError>> + Send;
    fn get_committee_info_by_validator_address(
        &self,
        epoch: Epoch,
        address: &Self::Addr,
    ) -> impl Future<Output = Result<CommitteeInfo, EpochManagerError>> + Send;
    fn get_committee_for_substate(
        &self,
        epoch: Epoch,
        substate_address: SubstateAddress,
    ) -> impl Future<Output = Result<Committee<Self::Addr>, EpochManagerError>> + Send;

    fn get_validator_node_by_public_key(
        &self,
        epoch: Epoch,
        public_key: RistrettoPublicKeyBytes,
    ) -> impl Future<Output = Result<ValidatorNode<Self::Addr>, EpochManagerError>> + Send;

    fn get_our_validator_node(
        &self,
        epoch: Epoch,
    ) -> impl Future<Output = Result<ValidatorNode<Self::Addr>, EpochManagerError>> + Send;

    fn get_local_committee_info(
        &self,
        epoch: Epoch,
    ) -> impl Future<Output = Result<CommitteeInfo, EpochManagerError>> + Send;

    fn get_committee_info(
        &self,
        epoch: Epoch,
        shard_group: ShardGroup,
    ) -> impl Future<Output = Result<CommitteeInfo, EpochManagerError>> + Send;

    fn get_committee_info_for_substate(
        &self,
        epoch: Epoch,
        shard: SubstateAddress,
    ) -> impl Future<Output = Result<CommitteeInfo, EpochManagerError>> + Send;

    fn get_committee_info_by_validator_public_key(
        &self,
        epoch: Epoch,
        public_key: RistrettoPublicKeyBytes,
    ) -> impl Future<Output = Result<CommitteeInfo, EpochManagerError>> + Send {
        async move {
            let validator = self.get_validator_node_by_public_key(epoch, public_key).await?;
            self.get_committee_info_for_substate(epoch, validator.shard_key).await
        }
    }

    fn current_epoch(&self) -> impl Future<Output = Result<Epoch, EpochManagerError>> + Send;
    fn get_current_epoch_hash(&self) -> impl Future<Output = Result<FixedHash, EpochManagerError>> + Send;

    fn get_num_committees(&self, epoch: Epoch) -> impl Future<Output = Result<u32, EpochManagerError>> + Send;

    fn get_committee_by_shard_group(
        &self,
        epoch: Epoch,
        shards: ShardGroup,
        limit: Option<usize>,
    ) -> impl Future<Output = Result<Committee<Self::Addr>, EpochManagerError>> + Send;
    fn get_committees_overlapping_shard_group(
        &self,
        epoch: Epoch,
        shard_group: ShardGroup,
    ) -> impl Future<Output = Result<HashMap<ShardGroup, Committee<Self::Addr>>, EpochManagerError>> + Send;

    fn get_local_committee(
        &self,
        epoch: Epoch,
    ) -> impl Future<Output = Result<Committee<Self::Addr>, EpochManagerError>> + Send {
        async move {
            let validator = self.get_our_validator_node(epoch).await?;
            let committee = self.get_committee_for_substate(epoch, validator.shard_key).await?;
            Ok(committee)
        }
    }

    fn get_committee_by_validator_public_key(
        &self,
        epoch: Epoch,
        public_key: RistrettoPublicKeyBytes,
    ) -> impl Future<Output = Result<Committee<Self::Addr>, EpochManagerError>> + Send {
        async move {
            let validator = self.get_validator_node_by_public_key(epoch, public_key).await?;
            let committee = self.get_committee_for_substate(epoch, validator.shard_key).await?;
            Ok(committee)
        }
    }

    /// Returns true if the validator is in the local committee for the given epoch.
    /// It is recommended that implementations override this method if they can provide a more efficient implementation.
    fn is_validator_in_local_committee(
        &self,
        validator_addr: &Self::Addr,
        epoch: Epoch,
    ) -> impl Future<Output = Result<bool, EpochManagerError>> + Send {
        async move {
            let committee = self.get_local_committee(epoch).await?;
            Ok(committee.contains(validator_addr))
        }
    }

    fn get_current_epoch_committee(
        &self,
        shard: SubstateAddress,
    ) -> impl Future<Output = Result<Committee<Self::Addr>, EpochManagerError>> + Send {
        async move {
            let current_epoch = self.current_epoch().await?;
            self.get_committee_for_substate(current_epoch, shard).await
        }
    }

    fn is_this_validator_registered_for_epoch(
        &self,
        epoch: Epoch,
    ) -> impl Future<Output = Result<bool, EpochManagerError>> + Send {
        async move {
            let current = self.current_epoch().await?;
            if current < epoch {
                return Ok(false);
            }

            // TODO: might want to improve this
            self.get_local_committee_info(epoch).await.map(|_| true).or_else(|err| {
                if err.is_not_registered_error() {
                    Ok(false)
                } else {
                    Err(err)
                }
            })
        }
    }

    fn add_intent_to_evict_validator(
        &self,
        proof: EvictionProof,
    ) -> impl Future<Output = Result<(), EpochManagerError>> + Send;

    fn get_random_committee_member(
        &self,
        epoch: Epoch,
        shard_group: Option<ShardGroup>,
        excluding: Vec<Self::Addr>,
    ) -> impl Future<Output = Result<ValidatorNode<Self::Addr>, EpochManagerError>> + Send;
}

pub trait EpochUtxoStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    fn add_unclaimed_utxo(&mut self, epoch: Epoch, substate: UnclaimedConfidentialOutput) -> Result<(), Self::Error>;
}

pub trait LayerOneTransactionSubmitter {
    type Output;
    type Error: std::error::Error;
    fn submit_transaction<T: serde::Serialize + Send>(
        &self,
        transaction: LayerOneTransactionDef<T>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send;
}

pub trait TemplateDownloader {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Enqueues the template for download. An implementation should not wait for the download to complete before
    /// resolving the returned future.
    fn enqueue_download(
        &mut self,
        epoch: Epoch,
        name: String,
        address: TemplateAddress,
        author_public_key: RistrettoPublicKeyBytes,
        url: Url,
        binary_hash: FixedHash,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
