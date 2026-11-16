//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    hash::Hash,
    sync::{Arc, Mutex},
};

use indexmap::IndexMap;
use log::{info, warn};
use tari_engine_types::limits::MAX_CBOR_NESTING_DEPTH;
use tari_epoch_manager::{EpochManagerError, EpochManagerReader};
use tari_ootle_common_types::{
    Epoch,
    NodeAddressable,
    NumPreshards,
    ShardGroup,
    SubstateAddress,
    ToSubstateAddress,
    committee::Committee,
    displayable::Displayable,
    optional::Optional,
};
use tari_ootle_storage::consensus_models::{CommittedBlockProof, CommittedBlockProofError, FinalizedTransactionLeaf};
use tari_ootle_transaction::{Network, Transaction, TransactionId};
use tari_rpc_framework::RpcStatusCode;
use tari_sidechain::FixedHash;
use tari_state_tree::{JmtHashScheme, JmtProofVerifyError, SparseMerkleProofExt, TreeHash};
use tari_validator_node_rpc::{
    ValidatorNodeRpcClientError,
    client::{FinalizedResult, TransactionResultStatus, ValidatorNodeClientFactory, ValidatorNodeRpcClient},
};

const LOG_TARGET: &str = "tari::indexer::network_client";

/// How many verified finalized results are remembered, so that a later poll of the same transaction is answered from
/// memory.
const VERIFIED_RESULTS_CAPACITY: usize = 4096;

/// How many verified block headers are remembered, so that the result of another transaction a remembered block
/// finalized is verified by its Merkle path alone.
const VERIFIED_BLOCKS_CAPACITY: usize = 1024;

#[derive(Debug, Clone)]
pub struct TariNetworkClient<TEpochManager, TClientFactory> {
    epoch_manager: TEpochManager,
    client_provider: TClientFactory,
    num_preshards: NumPreshards,
    network: Network,
    /// Finalized results whose decision verified, most recent last, bounded at [`VERIFIED_RESULTS_CAPACITY`].
    verified_results: Arc<Mutex<IndexMap<TransactionId, Box<FinalizedResult>>>>,
    /// The transaction merkle root of each block whose commit proof verified, by block id, most recent last, bounded
    /// at [`VERIFIED_BLOCKS_CAPACITY`].
    verified_blocks: Arc<Mutex<IndexMap<FixedHash, TreeHash>>>,
}

impl<TAddr, TEpochManager, TClientFactory> TariNetworkClient<TEpochManager, TClientFactory>
where
    TAddr: NodeAddressable + 'static,
    TEpochManager: EpochManagerReader<Addr = TAddr> + 'static,
    TClientFactory: ValidatorNodeClientFactory<TAddr> + 'static,
{
    pub fn new(
        epoch_manager: TEpochManager,
        client_provider: TClientFactory,
        num_preshards: NumPreshards,
        network: Network,
    ) -> Self {
        Self {
            epoch_manager,
            client_provider,
            num_preshards,
            network,
            verified_results: Arc::new(Mutex::new(IndexMap::with_capacity(VERIFIED_RESULTS_CAPACITY))),
            verified_blocks: Arc::new(Mutex::new(IndexMap::with_capacity(VERIFIED_BLOCKS_CAPACITY))),
        }
    }

    /// The current epoch, once the epoch manager has completed its initial scan. The scan is waited
    /// on because until it lands the epoch manager reports zero, and a caller deriving anything
    /// durable from a zero epoch — a retention key, a validity window — writes a value the node
    /// will disagree with seconds later.
    pub async fn current_epoch(&self) -> Result<Epoch, NetworkClientError> {
        self.epoch_manager.wait_for_initial_scanning_to_complete().await?;
        Ok(self.epoch_manager.current_epoch().await?)
    }

    pub async fn submit_transaction(&self, transaction: Transaction) -> Result<TransactionId, NetworkClientError> {
        // Ensure initial scanning has completed to ensure an accurate epoch
        self.epoch_manager.wait_for_initial_scanning_to_complete().await?;

        let tx_id = transaction.calculate_id();

        info!(
            target: LOG_TARGET,
            "Submitting transaction {} to the network", tx_id
        );

        let involved = transaction.involved_substate_addresses_iter().collect::<HashSet<_>>();

        let results = self
            .try_with_committee(involved, |mut client| {
                let transaction = transaction.clone();
                async move { client.submit_transaction(transaction).await }
            })
            .await?;

        let success_count = results.values().filter(|r| r.is_ok()).count();

        info!(
            target: LOG_TARGET,
            "Submitted transaction {} succeeded for {}/{} shard groups",
            tx_id,
            success_count,
            results.len()
        );
        if success_count != results.len() {
            warn!(
                target: LOG_TARGET,
                "Transaction {} was not submitted to some shard groups. {}",
                tx_id,
                results
                    .iter()
                    .filter_map(|(shard_group, result)| {
                        if let Err(err) = result {
                            Some(format!("Failed for {}: {}", shard_group, err))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        Ok(tx_id)
    }

    /// The status of a transaction as one member of its receipt committee reports it, or `None` when that member
    /// reports the transaction unknown.
    ///
    /// A finalized result is returned once its decision verifies against the quorum-signed header of the block that
    /// finalized it, so members are asked in turn until one serves a result that does. The rest of the result, its
    /// execution and abort details, is the serving member's account. Pending and unknown answers carry no proof, and
    /// the first member to give one is returned: each query asks the members in a fresh random order, so a member that
    /// misreports one only delays the result. A transaction whose finalized result this indexer cannot verify from any
    /// member, because no member holds a proof or this indexer knows no committee for the block a proof cites, is
    /// reported as pending.
    pub async fn get_finalized_transaction_result(
        &self,
        transaction_id: TransactionId,
    ) -> Result<Option<TransactionResultStatus>, NetworkClientError> {
        if let Some(result) = recall(&self.verified_results, &transaction_id) {
            return Ok(Some(TransactionResultStatus::Finalized(result)));
        }
        let committee = self
            .committee_for_substate(transaction_id.to_substate_address())
            .await?;

        let mut unverifiable = false;
        let mut last_error = None;
        for member in committee.shuffled() {
            let mut client = self.client_provider.create_client(&member.address);
            let finalized = match client.get_finalized_transaction_result(transaction_id).await.optional() {
                Ok(None) => return Ok(None),
                Ok(Some(TransactionResultStatus::Pending)) => return Ok(Some(TransactionResultStatus::Pending)),
                Ok(Some(TransactionResultStatus::Finalized(finalized))) => finalized,
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        "Validator '{member}' gave no usable result for transaction {transaction_id}: {err}"
                    );
                    last_error = Some(err);
                    continue;
                },
            };
            match self.verify_finalized_result(&transaction_id, &finalized).await {
                Ok(()) => {
                    remember(
                        &self.verified_results,
                        transaction_id,
                        finalized.clone(),
                        VERIFIED_RESULTS_CAPACITY,
                    );
                    return Ok(Some(TransactionResultStatus::Finalized(finalized)));
                },
                Err(ResultProofError::Unproven) => {
                    info!(
                        target: LOG_TARGET,
                        "Validator '{member}' holds no proof of its finalized result for transaction {transaction_id}"
                    );
                    unverifiable = true;
                },
                Err(err @ ResultProofError::CommitteeUnknown { .. }) => {
                    warn!(
                        target: LOG_TARGET,
                        "Cannot verify validator '{member}''s finalized result for transaction {transaction_id}: {err}"
                    );
                    unverifiable = true;
                },
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        "Validator '{member}' served a finalized result for transaction {transaction_id} that does not verify: {err}"
                    );
                    last_error = Some(ValidatorNodeRpcClientError::InvalidResponse(err.into()));
                },
            }
        }

        if unverifiable {
            warn!(
                target: LOG_TARGET,
                "No validator's finalized result for transaction {transaction_id} could be verified. Reporting it as pending."
            );
            return Ok(Some(TransactionResultStatus::Pending));
        }
        Err(NetworkClientError::AllValidatorsFailed {
            committee_size: committee.len(),
            last_error,
        })
    }

    /// Verifies that the block `finalized`'s proof cites finalized its decision for `transaction_id`, and that the
    /// committee of that block's epoch and shard group committed the block.
    async fn verify_finalized_result(
        &self,
        transaction_id: &TransactionId,
        finalized: &FinalizedResult,
    ) -> Result<(), ResultProofError> {
        check_decision_matches_execution(finalized)?;
        let proof = finalized.inclusion_proof.as_ref().ok_or(ResultProofError::Unproven)?;
        let commit_proof = CommittedBlockProof::from_bytes(&proof.commit_proof)?;
        let transaction_proof: SparseMerkleProofExt =
            tari_bor::serde_codec::from_slice_with_max_depth(&proof.transaction_proof, MAX_CBOR_NESTING_DEPTH)
                .map_err(|e| ResultProofError::UndecodableTransactionProof(e.to_string()))?;
        let root = self.verified_transaction_root(&commit_proof).await?;
        let leaf = FinalizedTransactionLeaf {
            transaction_id,
            decision: finalized.final_decision,
        };
        transaction_proof.verify_inclusion(JmtHashScheme::V1, &root, &leaf.key(), &leaf.hash())?;
        Ok(())
    }

    /// The transaction merkle root of the block `commit_proof` proves, once the committee of the block's epoch and
    /// shard group is verified to have committed it. A block id is the hash of its header, root included, so a root
    /// remembered by block id stands for any later proof of the same block.
    async fn verified_transaction_root(
        &self,
        commit_proof: &CommittedBlockProof,
    ) -> Result<TreeHash, ResultProofError> {
        let block_id = commit_proof.block_id();
        if let Some(root) = recall(&self.verified_blocks, &block_id) {
            return Ok(root);
        }
        let epoch = commit_proof.epoch();
        let shard_group = commit_proof.shard_group()?;
        let committee = self
            .epoch_manager
            .get_committee_by_shard_group(epoch, shard_group)
            .await
            .map_err(|source| ResultProofError::CommitteeUnknown {
                epoch,
                shard_group,
                source,
            })?;
        commit_proof.validate(self.network, committee.quorum_threshold(), &*committee)?;
        let root = TreeHash::from(commit_proof.proof().header.transaction_merkle_root.into_array());
        remember(&self.verified_blocks, block_id, root, VERIFIED_BLOCKS_CAPACITY);
        Ok(root)
    }

    async fn committee_for_substate(
        &self,
        substate_address: SubstateAddress,
    ) -> Result<Arc<Committee<TAddr>>, NetworkClientError> {
        let epoch = self.epoch_manager.current_epoch().await?;
        let num_committees = self.epoch_manager.get_num_committees(epoch).await?;
        let shard_group = substate_address.to_shard_group(self.num_preshards, num_committees);
        self.committee_for_shard_group(epoch, shard_group).await
    }

    /// A shard group with no assigned validators is a statement about the network - nothing answers
    /// for that part of the shard space at this epoch - rather than a failure of this indexer, so it
    /// must reach the caller as an unavailable committee and not an internal error.
    async fn committee_for_shard_group(
        &self,
        epoch: Epoch,
        shard_group: ShardGroup,
    ) -> Result<Arc<Committee<TAddr>>, NetworkClientError> {
        self.epoch_manager
            .get_committee_by_shard_group(epoch, shard_group)
            .await
            .optional()?
            .filter(|committee| !committee.is_empty())
            .ok_or(NetworkClientError::NoCommitteeForShardGroup { epoch, shard_group })
    }

    /// Fetches the committee members for the given shard and calls the given callback with each member until
    /// the callback returns a `Ok` with results for each shard group. If an Ok is returned, the hashmap is guaranteed
    /// to be the same size as the number of unique shard groups queried.
    pub async fn try_with_committee<'a, F, T, TFut, ISubstateAddr>(
        &self,
        substate_addresses: ISubstateAddr,
        mut callback: F,
    ) -> Result<IndexMap<ShardGroup, Result<T, ValidatorNodeRpcClientError>>, NetworkClientError>
    where
        F: FnMut(TClientFactory::Client) -> TFut,
        TFut: Future<Output = Result<T, ValidatorNodeRpcClientError>> + 'a,
        TClientFactory::Client: 'a,
        T: 'static,
        ISubstateAddr: IntoIterator<Item = SubstateAddress>,
    {
        let epoch = self.epoch_manager.current_epoch().await?;
        let num_committees = self.epoch_manager.get_num_committees(epoch).await?;

        info!(
            target: LOG_TARGET,
            "Fetching committee members at epoch {} ({} total committees)",
            epoch,
            num_committees,
        );

        let mut all_members = HashMap::new();
        for substate_address in substate_addresses {
            let shard_group = substate_address.to_shard_group(self.num_preshards, num_committees);

            if all_members.contains_key(&shard_group) {
                continue; // Already processed this shard group
            }

            let committee = self.committee_for_shard_group(epoch, shard_group).await?;
            all_members.insert(shard_group, committee);
        }

        let committee_size = all_members.len();
        if committee_size == 0 {
            return Err(NetworkClientError::NoCommitteeMembers);
        }

        let mut num_succeeded = 0;
        let mut results = IndexMap::with_capacity(committee_size);
        let mut last_error_sg = None;
        for (shard_group, committee) in all_members {
            for member in committee.shuffled() {
                let client = self.client_provider.create_client(&member.address);
                match callback(client).await {
                    Ok(ret) => {
                        num_succeeded += 1;
                        results.insert(shard_group, Ok(ret));
                        break; // Move onto the next shard group
                    },
                    Err(err) => {
                        warn!(
                            target: LOG_TARGET,
                            "Request failed for validator '{}': {}", member, err
                        );
                        last_error_sg = Some(shard_group);
                        results.insert(shard_group, Err(err));
                    },
                }
            }
        }

        if num_succeeded == 0 {
            let mut last_error = None;

            if let Some(sg) = last_error_sg {
                let last = results.swap_remove(&sg).expect("shard group must exist");
                match last {
                    Ok(_) => {},
                    Err(e) => last_error = Some(e),
                }
            }
            return Err(NetworkClientError::AllValidatorsFailed {
                committee_size,
                last_error,
            });
        }

        Ok(results)
    }
}

/// A member derives its decision from its execution result, so a result in which the two disagree cannot have come
/// from an honest member.
fn check_decision_matches_execution(finalized: &FinalizedResult) -> Result<(), ResultProofError> {
    let execute_result = finalized
        .execute_result
        .as_ref()
        .map(|r| r.decode())
        .transpose()
        .map_err(|e| ResultProofError::UndecodableExecutionResult(e.to_string()))?;
    let is_commit = finalized.final_decision.is_commit();
    match execute_result {
        Some(r) if r.finalize.result.is_any_accept() != is_commit => {
            Err(ResultProofError::DecisionContradictsExecution)
        },
        None if is_commit => Err(ResultProofError::CommitWithoutExecutionResult),
        _ => Ok(()),
    }
}

fn recall<K: Hash + Eq, V: Clone>(cache: &Mutex<IndexMap<K, V>>, key: &K) -> Option<V> {
    cache.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned()
}

/// Inserts `value` at `key`, evicting the oldest entry once `cache` holds more than `capacity`.
fn remember<K: Hash + Eq, V>(cache: &Mutex<IndexMap<K, V>>, key: K, value: V, capacity: usize) {
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    cache.insert(key, value);
    if cache.len() > capacity {
        cache.shift_remove_index(0);
    }
}

/// Why a member's finalized result is rejected.
#[derive(Debug, thiserror::Error)]
enum ResultProofError {
    #[error("the validator holds no proof of the result")]
    Unproven,
    #[error("the execution result does not decode: {0}")]
    UndecodableExecutionResult(String),
    #[error("the decision contradicts the execution result")]
    DecisionContradictsExecution,
    #[error("the commit decision has no execution result")]
    CommitWithoutExecutionResult,
    #[error("the transaction proof does not decode: {0}")]
    UndecodableTransactionProof(String),
    #[error("the commit proof of the finalizing block is invalid: {0}")]
    InvalidCommitProof(#[from] CommittedBlockProofError),
    #[error(
        "this indexer knows no committee of {shard_group} at {epoch}, which committed the finalizing block: {source}"
    )]
    CommitteeUnknown {
        epoch: Epoch,
        shard_group: ShardGroup,
        source: EpochManagerError,
    },
    #[error("the finalizing block did not finalize this decision: {0}")]
    DecisionNotFinalized(#[from] JmtProofVerifyError),
}

#[derive(Debug, thiserror::Error)]
pub enum NetworkClientError {
    #[error("Epoch manager error: {0}")]
    EpochManagerError(#[from] EpochManagerError),
    #[error("Rpc call failed for all ({committee_size}) validators: {}", .last_error.display())]
    AllValidatorsFailed {
        committee_size: usize,
        last_error: Option<ValidatorNodeRpcClientError>,
    },
    #[error("No committee at present. Try again later")]
    NoCommitteeMembers,
    #[error("No validators are assigned to {shard_group} at {epoch}")]
    NoCommitteeForShardGroup { epoch: Epoch, shard_group: ShardGroup },
}

impl NetworkClientError {
    /// Returns the rejection details when every involved validator failed and the last failure was a
    /// BAD_REQUEST, i.e. the transaction was explicitly rejected as invalid (e.g. by mempool
    /// validation) rather than failing for transient reasons.
    pub fn validation_rejection_details(&self) -> Option<&str> {
        let NetworkClientError::AllValidatorsFailed {
            last_error: Some(rpc_err),
            ..
        } = self
        else {
            return None;
        };
        let status = rpc_err.status()?;
        if status.as_status_code() == RpcStatusCode::BadRequest {
            Some(status.details())
        } else {
            None
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tari_consensus_types::Decision;
    use tari_engine_types::{
        commit_result::{ExecuteResult, RejectReason, TransactionResult},
        encoded::Encoded,
        substate::SubstateId,
    };
    use tari_epoch_manager::EpochManagerEvent;
    use tari_ootle_common_types::{
        SubstateRequirementRef,
        ValidatorSetHash,
        VotePower,
        committee::{CommitteeInfo, CommitteeMember},
    };
    use tari_ootle_light_client::test_fixtures::{Signers, transaction_block_commit_proof};
    use tari_ootle_storage::{global::models::ValidatorNode, time::PrimitiveDateTime};
    use tari_rpc_framework::RpcStatus;
    use tari_state_tree::KeyedProofTree;
    use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
    use tari_validator_node_rpc::client::{
        SubstateBatch,
        SubstateProofData,
        SubstateResult,
        TransactionInclusionProof,
    };
    use tokio::sync::broadcast;

    use super::*;

    type Addr = String;

    #[derive(Clone)]
    pub(crate) enum Answer {
        NotFound,
        Pending,
        Finalized(Box<FinalizedResult>),
        Unreachable,
    }

    /// Answers for each committee member, by address, and how many requests they have served.
    #[derive(Clone)]
    pub(crate) struct FakeCommittee(Arc<Mutex<HashMap<Addr, Answer>>>, Arc<AtomicUsize>);

    pub(crate) struct FakeMember {
        committee: FakeCommittee,
        address: Addr,
    }

    impl ValidatorNodeClientFactory<Addr> for FakeCommittee {
        type Client = FakeMember;

        fn create_client(&self, address: &Addr) -> Self::Client {
            FakeMember {
                committee: self.clone(),
                address: address.clone(),
            }
        }
    }

    impl ValidatorNodeRpcClient<Addr> for FakeMember {
        async fn submit_transaction(
            &mut self,
            transaction: Transaction,
        ) -> Result<TransactionId, ValidatorNodeRpcClientError> {
            Ok(transaction.calculate_id())
        }

        async fn get_finalized_transaction_result(
            &mut self,
            _: TransactionId,
        ) -> Result<TransactionResultStatus, ValidatorNodeRpcClientError> {
            self.committee.1.fetch_add(1, Ordering::SeqCst);
            match &self.committee.0.lock().unwrap()[&self.address] {
                Answer::NotFound => Err(RpcStatus::not_found("unknown transaction").into()),
                Answer::Pending => Ok(TransactionResultStatus::Pending),
                Answer::Finalized(result) => Ok(TransactionResultStatus::Finalized(result.clone())),
                Answer::Unreachable => Err(ValidatorNodeRpcClientError::InvalidResponse(anyhow::anyhow!(
                    "unreachable"
                ))),
            }
        }

        async fn get_substate(
            &mut self,
            _: SubstateRequirementRef<'_>,
        ) -> Result<SubstateResult, ValidatorNodeRpcClientError> {
            unimplemented!()
        }

        async fn get_substate_with_proof(
            &mut self,
            _: SubstateRequirementRef<'_>,
        ) -> Result<(SubstateResult, Option<SubstateProofData>), ValidatorNodeRpcClientError> {
            unimplemented!()
        }

        async fn get_substates_batch(
            &mut self,
            _: &[&SubstateId],
            _: bool,
        ) -> Result<SubstateBatch, ValidatorNodeRpcClientError> {
            unimplemented!()
        }
    }

    /// Answers with the polled committee for the current epoch, with the committee of [`signers`] for [`PROOF_EPOCH`],
    /// counting those lookups, and with no committee for [`UNKNOWN_EPOCH`].
    pub(crate) struct FakeEpochManager {
        polled: Arc<Committee<Addr>>,
        signing: Arc<Committee<Addr>>,
        signing_lookups: Arc<AtomicUsize>,
    }

    impl EpochManagerReader for FakeEpochManager {
        type Addr = Addr;

        fn subscribe(&self) -> broadcast::Receiver<EpochManagerEvent> {
            unimplemented!()
        }

        async fn wait_for_initial_scanning_to_complete(&self) -> Result<(), EpochManagerError> {
            Ok(())
        }

        async fn get_all_validator_nodes(&self, _: Epoch) -> Result<Vec<ValidatorNode<Addr>>, EpochManagerError> {
            unimplemented!()
        }

        async fn get_committee_info_by_validator_address(
            &self,
            _: Epoch,
            _: &Addr,
        ) -> Result<CommitteeInfo, EpochManagerError> {
            unimplemented!()
        }

        async fn get_committee_for_substate(
            &self,
            _: Epoch,
            _: SubstateAddress,
        ) -> Result<Arc<Committee<Addr>>, EpochManagerError> {
            Ok(self.polled.clone())
        }

        async fn get_validator_node_by_public_key(
            &self,
            _: Epoch,
            _: RistrettoPublicKeyBytes,
        ) -> Result<ValidatorNode<Addr>, EpochManagerError> {
            unimplemented!()
        }

        async fn get_our_validator_node(&self, _: Epoch) -> Result<ValidatorNode<Addr>, EpochManagerError> {
            unimplemented!()
        }

        async fn get_local_committee_info(&self, _: Epoch) -> Result<CommitteeInfo, EpochManagerError> {
            unimplemented!()
        }

        async fn get_committee_info(&self, _: Epoch, _: ShardGroup) -> Result<CommitteeInfo, EpochManagerError> {
            unimplemented!()
        }

        async fn get_committee_info_for_substate(
            &self,
            _: Epoch,
            _: SubstateAddress,
        ) -> Result<CommitteeInfo, EpochManagerError> {
            unimplemented!()
        }

        async fn current_epoch(&self) -> Result<Epoch, EpochManagerError> {
            Ok(Epoch(1))
        }

        async fn get_current_epoch_hash(&self) -> Result<FixedHash, EpochManagerError> {
            unimplemented!()
        }

        async fn get_epoch_hash(&self, _: Epoch) -> Result<FixedHash, EpochManagerError> {
            unimplemented!()
        }

        async fn get_validator_set_hash(&self, _: Epoch) -> Result<ValidatorSetHash, EpochManagerError> {
            unimplemented!()
        }

        async fn get_validator_set_committee_proof(
            &self,
            _: Epoch,
            _: ShardGroup,
        ) -> Result<tari_state_tree::ValidatorSetCommitteeProof, EpochManagerError> {
            unimplemented!()
        }

        async fn get_num_committees(&self, _: Epoch) -> Result<u32, EpochManagerError> {
            Ok(1)
        }

        async fn get_committee_by_shard_group(
            &self,
            epoch: Epoch,
            _: ShardGroup,
        ) -> Result<Arc<Committee<Addr>>, EpochManagerError> {
            if epoch == UNKNOWN_EPOCH {
                return Err(EpochManagerError::NoEpochFound(epoch));
            }
            if epoch == PROOF_EPOCH {
                self.signing_lookups.fetch_add(1, Ordering::SeqCst);
                return Ok(self.signing.clone());
            }
            Ok(self.polled.clone())
        }

        async fn get_committees_overlapping_shard_group(
            &self,
            _: Epoch,
            _: ShardGroup,
        ) -> Result<HashMap<ShardGroup, Committee<Addr>>, EpochManagerError> {
            unimplemented!()
        }

        async fn get_random_committee_member(
            &self,
            _: Epoch,
            _: Option<ShardGroup>,
            _: HashSet<Addr>,
        ) -> Result<ValidatorNode<Addr>, EpochManagerError> {
            unimplemented!()
        }

        async fn lock_epoch(&self, _: Epoch) -> Result<(), EpochManagerError> {
            unimplemented!()
        }

        async fn get_observed_epoch_hash(&self, _: Epoch) -> Result<Option<FixedHash>, EpochManagerError> {
            unimplemented!()
        }

        async fn get_birthday_epoch(&self) -> Result<Option<Epoch>, EpochManagerError> {
            unimplemented!()
        }

        async fn get_pending_activation(
            &self,
        ) -> Result<Option<tari_epoch_manager::PendingActivation>, EpochManagerError> {
            unimplemented!()
        }
    }

    const NETWORK: Network = Network::LocalNet;

    /// The epoch of the blocks the fixtures prove a decision in.
    const PROOF_EPOCH: Epoch = Epoch(0);

    /// An epoch whose committee the fake epoch manager does not know.
    const UNKNOWN_EPOCH: Epoch = Epoch(9);

    fn transaction_id() -> TransactionId {
        TransactionId::from([7u8; 32])
    }

    fn other_transaction_id() -> TransactionId {
        TransactionId::from([8u8; 32])
    }

    /// The committee that commits every block the fixtures prove a decision in.
    fn signers() -> Signers {
        Signers::new(1, 4)
    }

    fn committee_of(signers: &Signers) -> Committee<Addr> {
        Committee::new(
            signers
                .committee()
                .iter()
                .enumerate()
                .map(|(n, member)| CommitteeMember {
                    address: format!("signer{n}"),
                    public_key: member.public_key,
                    vote_power: member.vote_power,
                })
                .collect(),
        )
    }

    /// A block of [`PROOF_EPOCH`] finalizing `decisions`, committed by `signers`.
    struct FinalizingBlock {
        tree: KeyedProofTree,
        commit_proof: Vec<u8>,
    }

    impl FinalizingBlock {
        fn new(decisions: &[(TransactionId, Decision)], signers: &Signers) -> Self {
            Self::at(PROOF_EPOCH, decisions, signers)
        }

        fn at(epoch: Epoch, decisions: &[(TransactionId, Decision)], signers: &Signers) -> Self {
            let tree = KeyedProofTree::build(decisions.iter().map(|(transaction_id, decision)| {
                let leaf = FinalizedTransactionLeaf {
                    transaction_id,
                    decision: *decision,
                };
                (leaf.key(), leaf.hash())
            }))
            .unwrap();
            let commit_proof = transaction_block_commit_proof(
                NETWORK,
                epoch,
                ShardGroup::all_shards(NumPreshards::P1),
                5,
                tree.root(),
                signers,
            );
            Self { tree, commit_proof }
        }

        /// `answer` with this block's proof of `transaction_id`'s leaf.
        fn prove(&self, answer: Answer, transaction_id: TransactionId) -> Answer {
            let Answer::Finalized(mut result) = answer else {
                panic!("only a finalized answer carries a proof");
            };
            let key = FinalizedTransactionLeaf {
                transaction_id: &transaction_id,
                decision: result.final_decision,
            }
            .key();
            let (_, proof) = self.tree.get_proof(&key).unwrap();
            result.inclusion_proof = Some(TransactionInclusionProof {
                transaction_proof: tari_bor::serde_codec::to_vec(&proof).unwrap(),
                commit_proof: self.commit_proof.clone(),
            });
            Answer::Finalized(result)
        }
    }

    /// `answer` proven by a block that `signers` commit, finalizing `decision` for the queried transaction.
    fn proven_as(answer: Answer, decision: Decision, signers: &Signers) -> Answer {
        FinalizingBlock::new(
            &[(transaction_id(), decision), (other_transaction_id(), Decision::Commit)],
            signers,
        )
        .prove(answer, transaction_id())
    }

    fn unproven_finalized(result: TransactionResult, total_fees_required: u64) -> Answer {
        let mut execute_result = ExecuteResult::new_rejected(
            transaction_id().as_hash(),
            RejectReason::ForeignPledgeInputConflict,
            Some(Epoch(1)),
        );
        execute_result.finalize.result = result;
        execute_result.finalize.total_fees_required = total_fees_required;
        Answer::Finalized(Box::new(FinalizedResult {
            final_decision: (&execute_result.finalize.result).into(),
            execute_result: Some(Encoded::encode(&execute_result).unwrap()),
            execution_time: Default::default(),
            finalized_time: PrimitiveDateTime::MIN,
            abort_details: None,
            inclusion_proof: None,
        }))
    }

    fn decision_of(answer: &Answer) -> Decision {
        let Answer::Finalized(result) = answer else {
            panic!("only a finalized answer has a decision");
        };
        result.final_decision
    }

    /// A finalized answer proven by a block [`signers`] commit.
    fn proven(answer: Answer) -> Answer {
        let decision = decision_of(&answer);
        proven_as(answer, decision, &signers())
    }

    /// A proven commit, told apart from other commits of the same transaction by `tag`.
    fn committed(tag: u64) -> Answer {
        proven(unproven_committed(tag))
    }

    fn unproven_committed(tag: u64) -> Answer {
        unproven_finalized(TransactionResult::Accept(Default::default()), tag)
    }

    fn unproven_aborted() -> Answer {
        unproven_finalized(TransactionResult::Reject(RejectReason::ForeignPledgeInputConflict), 0)
    }

    /// A proven abort of the queried transaction.
    fn aborted() -> Answer {
        aborted_of(transaction_id())
    }

    /// A proven abort of `transaction_id`.
    pub(crate) fn aborted_of(transaction_id: TransactionId) -> Answer {
        let answer = unproven_aborted();
        let decision = decision_of(&answer);
        FinalizingBlock::new(
            &[(transaction_id, decision), (other_transaction_id(), Decision::Commit)],
            &signers(),
        )
        .prove(answer, transaction_id)
    }

    /// A commit whose proof is committed by a committee other than the one of its block's epoch and shard group.
    fn forged_commit(tag: u64) -> Answer {
        proven_as(unproven_committed(tag), Decision::Commit, &Signers::new(2, 4))
    }

    async fn query(answers: Vec<Answer>) -> Result<Option<TransactionResultStatus>, NetworkClientError> {
        network(answers)
            .0
            .get_finalized_transaction_result(transaction_id())
            .await
    }

    /// A committee of equally weighted members, member `n` answering with `answers[n]`, and a count
    /// of the requests it serves.
    pub(crate) fn network(
        answers: Vec<Answer>,
    ) -> (TariNetworkClient<FakeEpochManager, FakeCommittee>, Arc<AtomicUsize>) {
        let members = (0..answers.len())
            .map(|n| CommitteeMember {
                address: format!("vn{n}"),
                public_key: RistrettoPublicKeyBytes::default(),
                vote_power: VotePower::of(1),
            })
            .collect();
        let answers = answers
            .into_iter()
            .enumerate()
            .map(|(n, answer)| (format!("vn{n}"), answer))
            .collect();
        let requests = Arc::new(AtomicUsize::new(0));
        let client = TariNetworkClient::new(
            FakeEpochManager {
                polled: Arc::new(Committee::new(members)),
                signing: Arc::new(committee_of(&signers())),
                signing_lookups: Arc::new(AtomicUsize::new(0)),
            },
            FakeCommittee(Arc::new(Mutex::new(answers)), requests.clone()),
            NumPreshards::P1,
            NETWORK,
        );
        (client, requests)
    }

    fn is_pending(status: &TransactionResultStatus) -> bool {
        matches!(status, TransactionResultStatus::Pending)
    }

    /// The tag of a committed answer.
    fn commit_tag(status: &TransactionResultStatus) -> Option<u64> {
        match status {
            TransactionResultStatus::Finalized(result) if result.final_decision.is_commit() => Some(
                result
                    .execute_result
                    .as_ref()?
                    .decode()
                    .unwrap()
                    .finalize
                    .total_fees_required,
            ),
            _ => None,
        }
    }

    fn is_abort(status: &TransactionResultStatus) -> bool {
        matches!(status, TransactionResultStatus::Finalized(result) if result.final_decision.is_abort())
    }

    // Members are asked in random order, so the tests that mix answers repeat to put each answer
    // in every position.
    const ROUNDS: usize = 20;

    #[tokio::test]
    async fn one_member_proving_its_result_settles_the_query() {
        for _ in 0..ROUNDS {
            let status = query(vec![
                committed(1),
                Answer::Unreachable,
                Answer::Unreachable,
                Answer::Unreachable,
            ])
            .await
            .unwrap()
            .unwrap();
            assert_eq!(commit_tag(&status), Some(1));

            let status = query(vec![aborted(), Answer::Unreachable, Answer::Unreachable])
                .await
                .unwrap()
                .unwrap();
            assert!(is_abort(&status));
        }
    }

    #[tokio::test]
    async fn members_are_asked_until_one_proves_its_result() {
        for _ in 0..ROUNDS {
            let status = query(vec![
                unproven_committed(1),
                forged_commit(2),
                committed(3),
                Answer::Unreachable,
            ])
            .await
            .unwrap()
            .unwrap();
            assert_eq!(commit_tag(&status), Some(3));
        }
    }

    #[tokio::test]
    async fn a_finalized_result_no_member_proves_is_pending() {
        for _ in 0..ROUNDS {
            let status = query(vec![unproven_committed(1), unproven_aborted(), Answer::Unreachable])
                .await
                .unwrap()
                .unwrap();
            assert!(is_pending(&status));
        }
    }

    #[tokio::test]
    async fn a_proof_whose_committee_is_unknown_here_is_pending() {
        let block = FinalizingBlock::at(UNKNOWN_EPOCH, &[(transaction_id(), Decision::Commit)], &signers());
        let status = query(vec![block.prove(unproven_committed(1), transaction_id())])
            .await
            .unwrap()
            .unwrap();
        assert!(is_pending(&status));
    }

    #[tokio::test]
    async fn a_proof_committed_by_another_committee_is_discarded() {
        let result = query(vec![forged_commit(1)]).await;
        assert!(matches!(result, Err(NetworkClientError::AllValidatorsFailed { .. })));
    }

    #[tokio::test]
    async fn a_proof_of_another_decision_is_discarded() {
        let abort = Decision::from(&TransactionResult::Reject(RejectReason::ForeignPledgeInputConflict));
        for answer in [
            proven_as(unproven_committed(1), abort, &signers()),
            proven_as(unproven_aborted(), Decision::Commit, &signers()),
        ] {
            let result = query(vec![answer]).await;
            assert!(matches!(result, Err(NetworkClientError::AllValidatorsFailed { .. })));
        }
    }

    #[tokio::test]
    async fn a_proven_decision_contradicting_its_own_result_is_discarded() {
        let Answer::Finalized(mut forged) = unproven_committed(1) else {
            unreachable!()
        };
        forged.final_decision = (&TransactionResult::Reject(RejectReason::ForeignPledgeInputConflict)).into();
        let result = query(vec![proven(Answer::Finalized(forged))]).await;
        assert!(matches!(result, Err(NetworkClientError::AllValidatorsFailed { .. })));
    }

    #[tokio::test]
    async fn one_member_settles_a_pending_or_unknown_transaction() {
        let status = query(vec![Answer::Pending]).await.unwrap().unwrap();
        assert!(is_pending(&status));
        assert!(query(vec![Answer::NotFound]).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn every_member_failing_is_an_error() {
        let result = query(vec![Answer::Unreachable, Answer::Unreachable]).await;
        assert!(matches!(result, Err(NetworkClientError::AllValidatorsFailed { .. })));
    }

    #[tokio::test]
    async fn a_verified_result_is_answered_without_asking_again() {
        let (client, requests) = network(vec![committed(2); 4]);
        let status = client
            .get_finalized_transaction_result(transaction_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(commit_tag(&status), Some(2));
        assert_eq!(requests.swap(0, Ordering::SeqCst), 1);

        let status = client
            .get_finalized_transaction_result(transaction_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(commit_tag(&status), Some(2));
        assert_eq!(requests.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_verified_block_proves_the_other_decisions_it_finalized() {
        let abort = Decision::from(&TransactionResult::Reject(RejectReason::ForeignPledgeInputConflict));
        let block = FinalizingBlock::new(
            &[(transaction_id(), Decision::Commit), (other_transaction_id(), abort)],
            &signers(),
        );
        let (client, _) = network(vec![block.prove(unproven_committed(1), transaction_id())]);
        let status = client
            .get_finalized_transaction_result(transaction_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(commit_tag(&status), Some(1));
        assert_eq!(client.epoch_manager.signing_lookups.load(Ordering::SeqCst), 1);

        client.client_provider.0.lock().unwrap().insert(
            "vn0".to_string(),
            block.prove(unproven_aborted(), other_transaction_id()),
        );
        let status = client
            .get_finalized_transaction_result(other_transaction_id())
            .await
            .unwrap()
            .unwrap();
        assert!(is_abort(&status));
        assert_eq!(client.epoch_manager.signing_lookups.load(Ordering::SeqCst), 1);
    }
}
