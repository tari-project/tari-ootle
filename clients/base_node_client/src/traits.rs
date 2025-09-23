//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::future::Future;

use minotari_app_grpc::tari_rpc::ValidatorNodeChange;
use tari_common_types::types::FixedHash;
use tari_node_components::blocks::BlockHeader;
use tari_ootle_common_types::{Epoch, SubstateAddress};
use tari_template_lib::prelude::RistrettoPublicKeyBytes;
use tari_transaction_components::transaction_components::CodeTemplateRegistration;

use crate::{
    error::BaseNodeClientError,
    types::{BaseLayerConsensusConstants, BaseLayerMetadata, BaseLayerValidatorNode, SideChainUtxos},
};

pub trait BaseNodeClient: Send + Sync + Clone {
    fn test_connection(&mut self) -> impl Future<Output = Result<(), BaseNodeClientError>> + Send;
    fn get_network(&mut self) -> impl Future<Output = Result<u8, BaseNodeClientError>> + Send;
    fn get_tip_info(&mut self) -> impl Future<Output = Result<BaseLayerMetadata, BaseNodeClientError>> + Send;
    fn get_validator_node_changes(
        &mut self,
        epoch: Epoch,
        sidechain_id: Option<&RistrettoPublicKeyBytes>,
    ) -> impl Future<Output = Result<Vec<ValidatorNodeChange>, BaseNodeClientError>> + Send;
    fn get_validator_nodes(
        &mut self,
        height: u64,
    ) -> impl Future<Output = Result<Vec<BaseLayerValidatorNode>, BaseNodeClientError>> + Send;
    fn get_shard_key(
        &mut self,
        epoch: Epoch,
        public_key: &RistrettoPublicKeyBytes,
    ) -> impl Future<Output = Result<Option<SubstateAddress>, BaseNodeClientError>> + Send;
    fn get_template_registrations(
        &mut self,
        start_hash: Option<FixedHash>,
        count: u64,
    ) -> impl Future<Output = Result<Vec<CodeTemplateRegistration>, BaseNodeClientError>> + Send;
    fn get_header_by_hash(
        &mut self,
        block_hash: &FixedHash,
    ) -> impl Future<Output = Result<BlockHeader, BaseNodeClientError>> + Send;
    fn get_consensus_constants(
        &mut self,
        tip: u64,
    ) -> impl Future<Output = Result<BaseLayerConsensusConstants, BaseNodeClientError>> + Send;
    fn get_sidechain_utxos(
        &mut self,
        start_hash: Option<FixedHash>,
        count: u64,
    ) -> impl Future<Output = Result<Vec<SideChainUtxos>, BaseNodeClientError>> + Send;
}
