//  Copyright 2022, The Tari Project
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

use std::time::Duration;

use log::*;
use reqwest::Url;
use tari_base_node_client::{
    grpc::GrpcBaseNodeClient,
    types::{BaseLayerMetadata, BlockInfo},
    BaseNodeClient,
    BaseNodeClientError,
};
use tari_common_types::types::{Commitment, FixedHash, FixedHashSizeError, PublicKey};
use tari_consensus::consensus_constants::ConsensusConstants;
use tari_core::{
    base_node::comms_interface::ValidatorNodeChange,
    transactions::{
        tari_amount::MicroMinotari,
        transaction_components::{
            CodeTemplateRegistration,
            SideChainFeatureData,
            TransactionOutput,
            ValidatorNodeRegistration,
        },
    },
};
use tari_crypto::{
    ristretto::RistrettoPublicKey,
    tari_utilities::{ByteArray, ByteArrayError},
};
use tari_dan_common_types::{
    option::DisplayContainer,
    optional::Optional,
    Epoch,
    NodeAddressable,
    VersionedSubstateId,
};
use tari_dan_storage::{
    consensus_models::{BurntUtxo, SubstateRecord},
    global::{GlobalDb, MetadataKey},
    StateStore,
    StorageError,
};
use tari_dan_storage_sqlite::{error::SqliteStorageError, global::SqliteGlobalDbAdapter};
use tari_engine_types::{confidential::UnclaimedConfidentialOutput, substate::SubstateId, TemplateAddress};
use tari_epoch_manager::{base_layer::EpochManagerHandle, EpochManagerError, EpochManagerReader};
use tari_shutdown::ShutdownSignal;
use tari_state_store_sqlite::SqliteStateStore;
use tari_template_lib::models::{EncryptedData, UnclaimedConfidentialOutputAddress};
use tokio::{task, task::JoinHandle, time};
use url::ParseError;

use crate::template_manager::interface::{TemplateExecutable, TemplateManagerError, TemplateManagerHandle};

const LOG_TARGET: &str = "tari::dan::base_layer_scanner";

pub fn spawn<TAddr: NodeAddressable + 'static>(
    global_db: GlobalDb<SqliteGlobalDbAdapter<TAddr>>,
    base_node_client: GrpcBaseNodeClient,
    epoch_manager: EpochManagerHandle<TAddr>,
    shutdown: ShutdownSignal,
    consensus_constants: ConsensusConstants,
    shard_store: SqliteStateStore<TAddr>,
    scan_base_layer: bool,
    base_layer_scanning_interval: Duration,
    validator_node_sidechain_id: Option<RistrettoPublicKey>,
    burnt_utxo_sidechain_id: Option<RistrettoPublicKey>,
    // TODO: remove when base layer template registration is removed too
    template_manager: TemplateManagerHandle,
    template_sidechain_id: Option<PublicKey>,
) -> JoinHandle<anyhow::Result<()>> {
    task::spawn(async move {
        let base_layer_scanner = BaseLayerScanner::new(
            global_db,
            base_node_client,
            epoch_manager,
            shutdown,
            consensus_constants,
            shard_store,
            scan_base_layer,
            base_layer_scanning_interval,
            validator_node_sidechain_id,
            burnt_utxo_sidechain_id,
            template_manager,
            template_sidechain_id,
        );

        base_layer_scanner.start().await?;
        Ok(())
    })
}

pub struct BaseLayerScanner<TAddr> {
    global_db: GlobalDb<SqliteGlobalDbAdapter<TAddr>>,
    last_scanned_height: u64,
    last_scanned_tip: Option<FixedHash>,
    last_scanned_hash: Option<FixedHash>,
    last_scanned_validator_node_mr: Option<FixedHash>,
    next_block_hash: Option<FixedHash>,
    base_node_client: GrpcBaseNodeClient,
    epoch_manager: EpochManagerHandle<TAddr>,
    shutdown: ShutdownSignal,
    consensus_constants: ConsensusConstants,
    state_store: SqliteStateStore<TAddr>,
    scan_base_layer: bool,
    base_layer_scanning_interval: Duration,
    has_attempted_scan: bool,
    validator_node_sidechain_id: Option<PublicKey>,
    burnt_utxo_sidechain_id: Option<PublicKey>,
    // TODO: remove template related data, when removed base layer template registration support
    template_manager: TemplateManagerHandle,
    template_sidechain_id: Option<PublicKey>,
}

impl<TAddr: NodeAddressable + 'static> BaseLayerScanner<TAddr> {
    pub fn new(
        global_db: GlobalDb<SqliteGlobalDbAdapter<TAddr>>,
        base_node_client: GrpcBaseNodeClient,
        epoch_manager: EpochManagerHandle<TAddr>,
        shutdown: ShutdownSignal,
        consensus_constants: ConsensusConstants,
        state_store: SqliteStateStore<TAddr>,
        scan_base_layer: bool,
        base_layer_scanning_interval: Duration,
        validator_node_sidechain_id: Option<PublicKey>,
        burnt_utxo_sidechain_id: Option<PublicKey>,
        template_manager: TemplateManagerHandle,
        template_sidechain_id: Option<PublicKey>,
    ) -> Self {
        Self {
            global_db,
            last_scanned_tip: None,
            last_scanned_height: 0,
            last_scanned_hash: None,
            last_scanned_validator_node_mr: None,
            next_block_hash: None,
            base_node_client,
            epoch_manager,
            shutdown,
            consensus_constants,
            state_store,
            scan_base_layer,
            base_layer_scanning_interval,
            has_attempted_scan: false,
            validator_node_sidechain_id,
            burnt_utxo_sidechain_id,
            template_manager,
            template_sidechain_id,
        }
    }

    pub async fn start(mut self) -> Result<(), BaseLayerScannerError> {
        if !self.scan_base_layer {
            info!(
                target: LOG_TARGET,
                "⚠️ scan_base_layer turned OFF. Base layer scanner is exiting."
            );
            return Ok(());
        }

        self.load_initial_state()?;
        // Scan on startup
        if let Err(err) = self.scan_blockchain().await {
            error!(target: LOG_TARGET, "Base layer scanner failed with error: {}", err);
        }

        loop {
            tokio::select! {
                _ = time::sleep(self.base_layer_scanning_interval) => {
                    if let Err(err) = self.scan_blockchain().await {
                        error!(target: LOG_TARGET, "Base layer scanner failed with error: {}", err);
                    }
                },
                _ = self.shutdown.wait() => break
            }
        }

        Ok(())
    }

    fn load_initial_state(&mut self) -> Result<(), BaseLayerScannerError> {
        let mut tx = self.global_db.create_transaction()?;
        let mut metadata = self.global_db.metadata(&mut tx);

        self.last_scanned_tip = metadata.get_metadata(MetadataKey::BaseLayerScannerLastScannedTip)?;
        self.last_scanned_hash = metadata.get_metadata(MetadataKey::BaseLayerScannerLastScannedBlockHash)?;
        self.last_scanned_height = metadata
            .get_metadata(MetadataKey::BaseLayerScannerLastScannedBlockHeight)?
            .unwrap_or(0);
        self.next_block_hash = metadata.get_metadata(MetadataKey::BaseLayerScannerNextBlockHash)?;
        Ok(())
    }

    async fn scan_blockchain(&mut self) -> Result<(), BaseLayerScannerError> {
        // fetch the new base layer info since the previous scan
        let tip = self.base_node_client.get_tip_info().await?;

        match self.get_blockchain_progression(&tip).await? {
            BlockchainProgression::Progressed => {
                info!(
                    target: LOG_TARGET,
                    "⛓️ Blockchain has progressed to height {}. We last scanned {}/{}. Scanning for new side-chain \
                     UTXOs.",
                    tip.height_of_longest_chain,
                    self.last_scanned_height,
                    tip.height_of_longest_chain
                        .saturating_sub(self.consensus_constants.base_layer_confirmations)
                );
                self.sync_blockchain(tip).await?;
            },
            BlockchainProgression::Reorged => {
                error!(
                    target: LOG_TARGET,
                    "⚠️ Base layer reorg detected. Rescanning from genesis."
                );
                // TODO: we need to figure out where the fork happened, and delete data after the fork.
                self.last_scanned_hash = None;
                self.last_scanned_validator_node_mr = None;
                self.last_scanned_height = 0;
                self.sync_blockchain(tip).await?;
            },
            BlockchainProgression::NoProgress => {
                trace!(target: LOG_TARGET, "No new blocks to scan.");
                // If no progress has been made since restarting, we still need to tell the epoch manager that scanning
                // is done
                if !self.has_attempted_scan {
                    self.epoch_manager.notify_scanning_complete().await?;
                }
            },
        }

        self.has_attempted_scan = true;

        Ok(())
    }

    async fn get_blockchain_progression(
        &mut self,
        tip: &BaseLayerMetadata,
    ) -> Result<BlockchainProgression, BaseLayerScannerError> {
        if tip.height_of_longest_chain == 0 {
            return Ok(BlockchainProgression::NoProgress);
        }
        match self.last_scanned_tip {
            Some(hash) if hash == tip.tip_hash => Ok(BlockchainProgression::NoProgress),
            Some(hash) => {
                let header = self.base_node_client.get_header_by_hash(hash).await.optional()?;
                if header.is_some() {
                    Ok(BlockchainProgression::Progressed)
                } else {
                    Ok(BlockchainProgression::Reorged)
                }
            },
            None => Ok(BlockchainProgression::Progressed),
        }
    }

    #[allow(clippy::too_many_lines)]
    async fn sync_blockchain(&mut self, tip: BaseLayerMetadata) -> Result<(), BaseLayerScannerError> {
        let start_scan_height = self.last_scanned_height;
        let mut current_hash = self.last_scanned_hash;
        let end_height = match tip
            .height_of_longest_chain
            .checked_sub(self.consensus_constants.base_layer_confirmations)
        {
            None => {
                debug!(
                    target: LOG_TARGET,
                    "Base layer blockchain is not yet at the required height to start scanning it"
                );
                return Ok(());
            },
            Some(end_height) => end_height,
        };

        // Recover the last scanned validator node MR if it is not set yet, i.e the node has scanned BL blocks
        // previously.
        if self.last_scanned_validator_node_mr.is_none() {
            if let Some(hash) = self.last_scanned_hash {
                let header = self.base_node_client.get_header_by_hash(hash).await?;
                self.last_scanned_validator_node_mr = Some(header.validator_node_mr);
            }
        }

        for current_height in start_scan_height..=end_height {
            let utxos = self
                .base_node_client
                .get_sidechain_utxos(current_hash, 1)
                .await?
                .pop()
                .ok_or_else(|| {
                    BaseLayerScannerError::InvalidSideChainUtxoResponse(format!(
                        "Base layer returned empty response for height {}",
                        current_height
                    ))
                })?;
            let block_info = utxos.block_info;

            // TODO: Because we don't know the next hash when we're done scanning to the tip, we need to load the
            //       previous scanned block again to get it.  This isn't ideal, but won't be an issue when we scan a few
            //       blocks back.
            if self.last_scanned_hash.is_some_and(|h| h == block_info.hash) {
                if let Some(hash) = block_info.next_block_hash {
                    current_hash = Some(hash);
                    continue;
                }
                break;
            }
            info!(
                target: LOG_TARGET,
                "⛓️ Scanning base layer block {} of {}", block_info.height, end_height
            );

            let header = self.base_node_client.get_header_by_hash(block_info.hash).await?;
            let current_validator_node_mr = header.validator_node_mr;
            self.epoch_manager
                .add_block_hash(header.height, block_info.hash)
                .await?;

            for output in utxos.outputs {
                let output_hash = output.hash();
                let Some(sidechain_feature) = output.features.sidechain_feature.as_ref() else {
                    warn!(target: LOG_TARGET, "Base node returned invalid data: Sidechain utxo output must have sidechain features");
                    continue;
                };
                match sidechain_feature.data() {
                    SideChainFeatureData::ValidatorNodeRegistration(reg) => {
                        if sidechain_feature.sidechain_public_key() != self.validator_node_sidechain_id.as_ref() {
                            debug!(
                                target: LOG_TARGET,
                                "Ignoring code template registration for sidechain ID {:?}.",
                                sidechain_feature.sidechain_public_key(),
                            );
                            continue;
                        }
                        trace!(target: LOG_TARGET, "New validator node registration scanned: {reg:?}");
                    },
                    // TODO: remove completely SideChainFeature::CodeTemplateRegistration at some point
                    SideChainFeatureData::CodeTemplateRegistration(reg) => {
                        if sidechain_feature.sidechain_public_key() != self.template_sidechain_id.as_ref() {
                            debug!(
                                target: LOG_TARGET,
                                "Ignoring code template registration for sidechain ID {:?}. Local node's sidechain ID: {:?}",
                                sidechain_feature.sidechain_public_key(),
                                self.template_sidechain_id,
                            );
                            continue;
                        }
                        self.register_code_template_registration(
                            reg.template_name.to_string(),
                            (*output_hash).into(),
                            reg.clone(),
                            &block_info,
                        )
                        .await?;
                    },
                    SideChainFeatureData::ConfidentialOutput(_) => {
                        // Should be checked by the base layer
                        if !output.is_burned() {
                            warn!(
                                target: LOG_TARGET,
                                "Ignoring confidential output that is not burned: {} with commitment {}",
                                output_hash,
                                output.commitment.as_public_key()
                            );
                            continue;
                        }
                        if sidechain_feature.sidechain_public_key() != self.burnt_utxo_sidechain_id.as_ref() {
                            debug!(
                                target: LOG_TARGET,
                                "Ignoring burnt UTXO for sidechain ID {:?}. Local node's sidechain ID: {:?}",
                                sidechain_feature.sidechain_public_key(),
                                self.burnt_utxo_sidechain_id,
                            );
                            continue;
                        }
                        info!(
                            target: LOG_TARGET,
                            "⛓️ Found burned output: {} with commitment {}",
                            output_hash,
                            output.commitment.as_public_key()
                        );
                        self.register_burnt_utxo(output, &block_info).await?;
                    },
                    SideChainFeatureData::EvictionProof(proof) => {
                        if sidechain_feature.sidechain_public_key() != self.validator_node_sidechain_id.as_ref() {
                            debug!(
                                target: LOG_TARGET,
                                "Ignoring eviction for sidechain ID {:?}. Local node's sidechain ID: {:?}",
                                sidechain_feature.sidechain_public_key(),
                                self.validator_node_sidechain_id,
                            );
                            continue;
                        }
                        trace!(target: LOG_TARGET, "Eviction proof scanned: {proof:?}");
                    },
                }
            }

            debug!(
                target: LOG_TARGET,
                "⛓️ last_scanned_validator_node_mr = {} current = {}", self.last_scanned_validator_node_mr.display(), current_validator_node_mr
            );
            // if the validator node MR has changed, we need to update the active validator node set
            if self
                .last_scanned_validator_node_mr
                .map_or(true, |last| last != current_validator_node_mr)
            {
                let constants = self.base_node_client.get_consensus_constants(block_info.height).await?;
                let scanned_epoch = constants.height_to_epoch(block_info.height);
                self.update_validators(scanned_epoch).await?;
                self.last_scanned_validator_node_mr = Some(current_validator_node_mr);
            }

            // Once we have all the UTXO and validator data, we "activate" the new epoch if applicable.
            self.epoch_manager
                .update_epoch(block_info.height, block_info.hash)
                .await?;

            self.set_last_scanned_block(tip.tip_hash, &block_info)?;

            match block_info.next_block_hash {
                Some(next_hash) => {
                    current_hash = Some(next_hash);
                },
                None => {
                    info!(
                        target: LOG_TARGET,
                        "⛓️ No more blocks to scan. Last scanned block height: {}", block_info.height
                    );
                    if block_info.height != end_height {
                        return Err(BaseLayerScannerError::InvalidSideChainUtxoResponse(format!(
                            "Expected to scan to height {}, but got to height {}",
                            end_height, block_info.height
                        )));
                    }
                    break;
                },
            }
        }

        self.epoch_manager.notify_scanning_complete().await?;

        Ok(())
    }

    async fn register_code_template_registration(
        &mut self,
        template_name: String,
        template_address: TemplateAddress,
        registration: CodeTemplateRegistration,
        block_info: &BlockInfo,
    ) -> Result<(), BaseLayerScannerError> {
        info!(
            target: LOG_TARGET,
            "🌠 new template found with address {} at height {}", template_address, block_info.height
        );
        self.template_manager
            .add_template(
                registration.author_public_key,
                template_address,
                TemplateExecutable::DownloadableWasm(
                    Url::parse(registration.binary_url.as_str())?,
                    registration.binary_sha,
                ),
                Some(template_name),
            )
            .await?;

        Ok(())
    }

    async fn update_validators(&mut self, epoch: Epoch) -> Result<(), BaseLayerScannerError> {
        info!(
            target: LOG_TARGET,
            "⛓️ Updating active validator node set (sidechain ID: {:?}) from base node for epoch {epoch}",
            self.validator_node_sidechain_id,
        );

        let node_changes = self
            .base_node_client
            .get_validator_node_changes(epoch, self.validator_node_sidechain_id.as_ref())
            .await
            .map_err(BaseLayerScannerError::BaseNodeError)?;

        info!(
            target: LOG_TARGET,
            "⛓️ {} validator node change(s) for epoch {}", node_changes.len(), epoch,
        );

        for node_change in node_changes {
            match node_change {
                ValidatorNodeChange::Add {
                    registration,
                    activation_epoch,
                    minimum_value_promise,
                } => {
                    self.add_validator_node_registration(
                        Epoch(activation_epoch.as_u64()),
                        registration,
                        minimum_value_promise,
                    )
                    .await?;
                },
                ValidatorNodeChange::Remove { public_key } => {
                    self.remove_validator_node_registration(public_key, epoch).await?;
                },
            }
        }

        Ok(())
    }

    async fn register_burnt_utxo(
        &mut self,
        output: TransactionOutput,
        block_info: &BlockInfo,
    ) -> Result<(), BaseLayerScannerError> {
        let commitment_address = UnclaimedConfidentialOutputAddress::try_from_commitment(output.commitment.as_bytes())
            .map_err(|e|
                // Technically impossible, but anyway
                BaseLayerScannerError::InvalidSideChainUtxoResponse(format!("Invalid commitment: {}", e)))?;
        let substate_id = SubstateId::UnclaimedConfidentialOutput(commitment_address);
        let consensus_constants = self.epoch_manager.get_base_layer_consensus_constants().await?;
        let epoch = consensus_constants.height_to_epoch(block_info.height);
        let Some(local_committee_info) = self.epoch_manager.get_local_committee_info(epoch).await.optional()? else {
            debug!(
                target: LOG_TARGET,
                "Validator node is not registered for the current epoch {epoch}. Ignoring burnt UTXO.",
            );
            return Ok(());
        };

        if !local_committee_info.includes_substate_id(&substate_id) {
            debug!(
                target: LOG_TARGET,
                "Validator node is not part of the committee for the burnt UTXO {substate_id}. Ignoring."
            );
            return Ok(());
        }

        let encrypted_data_bytes = output.encrypted_data.into_byte_vec();
        let encrypted_data = EncryptedData::try_from(encrypted_data_bytes).map_err(|len| {
            BaseLayerScannerError::InvalidSideChainUtxoResponse(format!(
                "Encrypted data incorrect length of bytes: {len}"
            ))
        })?;

        let substate = UnclaimedConfidentialOutput {
            commitment: output.commitment.clone(),
            encrypted_data,
        };

        info!(
            target: LOG_TARGET,
            "⛓️ Burnt UTXO {substate_id} registered at height {}",
            block_info.height,
        );

        self.state_store
            .with_write_tx(|tx| {
                if SubstateRecord::exists(&**tx, &VersionedSubstateId::new(substate_id.clone(), 0))? {
                    warn!(
                        target: LOG_TARGET,
                        "Burnt UTXO {substate_id} already exists. Ignoring.",
                    );
                    return Ok(());
                }

                BurntUtxo::new(commitment_address, substate, block_info.height).insert(tx)
            })
            .map_err(|source| BaseLayerScannerError::CouldNotRegisterBurntUtxo {
                commitment: Box::new(output.commitment),
                source,
            })?;

        Ok(())
    }

    async fn add_validator_node_registration(
        &mut self,
        activation_epoch: Epoch,
        registration: ValidatorNodeRegistration,
        minimum_value_promise: MicroMinotari,
    ) -> Result<(), BaseLayerScannerError> {
        info!(
            target: LOG_TARGET,
            "⛓️ Validator node {} activated at {}",
            registration.public_key(),
            activation_epoch,
        );

        self.epoch_manager
            .add_validator_node_registration(activation_epoch, registration, minimum_value_promise)
            .await?;

        Ok(())
    }

    async fn remove_validator_node_registration(
        &mut self,
        public_key: PublicKey,
        deactivation_epoch: Epoch,
    ) -> Result<(), BaseLayerScannerError> {
        info!(
            target: LOG_TARGET,
            "⛓️ Deactivating validator node registration for {}",
            public_key,
        );

        self.epoch_manager
            .deactivate_validator_node(public_key, deactivation_epoch)
            .await?;

        Ok(())
    }

    fn set_last_scanned_block(&mut self, tip: FixedHash, block_info: &BlockInfo) -> Result<(), BaseLayerScannerError> {
        let mut tx = self.global_db.create_transaction()?;
        let mut metadata = self.global_db.metadata(&mut tx);
        metadata.set_metadata(MetadataKey::BaseLayerScannerLastScannedTip, &tip)?;
        metadata.set_metadata(MetadataKey::BaseLayerScannerLastScannedBlockHash, &block_info.hash)?;
        metadata.set_metadata(MetadataKey::BaseLayerScannerNextBlockHash, &block_info.next_block_hash)?;
        metadata.set_metadata(MetadataKey::BaseLayerScannerLastScannedBlockHeight, &block_info.height)?;
        self.global_db.commit(tx)?;
        self.last_scanned_tip = Some(tip);
        self.last_scanned_hash = Some(block_info.hash);
        self.next_block_hash = block_info.next_block_hash;
        self.last_scanned_height = block_info.height;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BaseLayerScannerError {
    #[error(transparent)]
    FixedHashSizeError(#[from] FixedHashSizeError),
    #[error("Storage error: {0}")]
    SqliteStorageError(#[from] SqliteStorageError),
    #[error("Epoch manager error: {0}")]
    EpochManagerError(#[from] EpochManagerError),
    #[error("Base node client error: {0}")]
    BaseNodeError(#[from] BaseNodeClientError),
    #[error("Invalid side chain utxo response: {0}")]
    InvalidSideChainUtxoResponse(String),
    #[error("Could not register burnt UTXO because {source}")]
    CouldNotRegisterBurntUtxo {
        commitment: Box<Commitment>,
        source: StorageError,
    },
    #[error("Public key conversion error: {0}")]
    PublicKeyConversion(ByteArrayError),
    #[error("GRPC conversion error: {0}")]
    GrpcConversion(String),
    #[error("Template manager error: {0}")]
    TemplateManagerError(#[from] TemplateManagerError),
    #[error("URL parse error: {0}")]
    UrlParse(#[from] ParseError),
}

enum BlockchainProgression {
    /// The blockchain has progressed since the last scan
    Progressed,
    /// Reorg was detected
    Reorged,
    /// The blockchain has not progressed since the last scan
    NoProgress,
}
