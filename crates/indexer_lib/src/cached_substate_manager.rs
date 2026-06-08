//  Copyright 2023, The Tari Project
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
    collections::HashMap,
    time::{Duration, SystemTime},
};

use log::*;
use tari_engine_types::substate::{Substate, SubstateId, SubstateValue};
use tari_epoch_manager::EpochManagerReader;
use tari_ootle_common_types::{
    Epoch,
    NodeAddressable,
    NumPreshards,
    ShardGroup,
    SubstateAddress,
    SubstateRequirementRef,
    ToSubstateAddress,
    VotePower,
    displayable::Displayable,
};
use tari_ootle_storage::{consensus_models::CommittedBlockProof, verify_substate_value_proof};
use tari_validator_node_rpc::client::{
    SubstateProofData,
    SubstateResult,
    ValidatorNodeClientFactory,
    ValidatorNodeRpcClient,
};

use crate::{
    error::IndexerError,
    substate_cache::{SubstateCache, SubstateCacheEntry, SubstateCacheEntryRef},
};

const LOG_TARGET: &str = "tari::indexer::scanner";

const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub struct CachedSubstateManager<TEpochManager, TVnClient, TSubstateCache> {
    committee_provider: TEpochManager,
    validator_node_client_factory: TVnClient,
    substate_cache: TSubstateCache,
    cache_ttl: Duration,
    /// When set, substates fetched from a validator must come with a proof that verifies against the
    /// shard group committee, or they are rejected (fail-closed). The negative `DoesNotExist` case
    /// is not provable and is left to the existing f+1 agreement.
    verify_substate_proofs: bool,
    #[cfg(feature = "metrics")]
    metrics: Option<crate::metrics::Metrics>,
}

impl<TEpochManager, TVnClient, TAddr, TSubstateCache> CachedSubstateManager<TEpochManager, TVnClient, TSubstateCache>
where
    TAddr: NodeAddressable,
    TEpochManager: EpochManagerReader<Addr = TAddr>,
    TVnClient: ValidatorNodeClientFactory<TAddr>,
    TSubstateCache: SubstateCache,
{
    pub fn new(
        committee_provider: TEpochManager,
        validator_node_client_factory: TVnClient,
        substate_cache: TSubstateCache,
    ) -> Self {
        Self {
            committee_provider,
            validator_node_client_factory,
            substate_cache,
            cache_ttl: DEFAULT_CACHE_TTL,
            verify_substate_proofs: false,
            #[cfg(feature = "metrics")]
            metrics: None,
        }
    }

    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = ttl;
        self
    }

    pub fn with_substate_proof_verification(mut self, enabled: bool) -> Self {
        self.verify_substate_proofs = enabled;
        self
    }

    /// Whether substates served by this manager are verified against the shard group committee.
    pub fn verifies_substates(&self) -> bool {
        self.verify_substate_proofs
    }

    #[cfg(feature = "metrics")]
    pub fn with_metrics(mut self, registry: &mut prometheus_client::registry::Registry) -> Self {
        self.metrics = Some(crate::metrics::Metrics::register(registry));
        self
    }

    /// Attempts to find the latest substate for the given address. If the lowest possible version is known, it can be
    /// provided to reduce effort/time required to scan.
    pub async fn get_substate(
        &self,
        substate_id: &SubstateId,
        specific_version: Option<u32>,
    ) -> Result<SubstateResult, IndexerError> {
        debug!(target: LOG_TARGET, "get_substate: {}v{}", substate_id, specific_version.display());
        let mut cached_version = None;
        // start from the latest cached version of the substate (if cached previously)
        let cache_res = self.substate_cache.read(substate_id).await?;
        if let Some(entry) = cache_res {
            if let Some(version) = specific_version {
                if entry.version == version {
                    debug!(target: LOG_TARGET, "Substate cache hit for {} with version {}", entry.version, substate_id);
                    #[cfg(feature = "metrics")]
                    self.metrics.as_ref().inspect(|m| m.inc_cache_hits());
                    return Ok(entry.substate_result);
                }
            } else {
                let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_secs();
                if now.saturating_sub(entry.cached_at) > self.cache_ttl.as_secs() {
                    debug!(target: LOG_TARGET, "Cached substate {} is stale. Fetching fresh copy.", substate_id);
                } else {
                    debug!(target: LOG_TARGET, "Substate cache hit for {} with version {}", substate_id, entry.version);
                    #[cfg(feature = "metrics")]
                    self.metrics.as_ref().inspect(|m| m.inc_cache_hits());
                    return Ok(entry.substate_result.clone());
                }
            }

            cached_version = Some(entry.version);
        }
        #[cfg(feature = "metrics")]
        self.metrics.as_ref().inspect(|m| m.inc_cache_misses());

        let substate_result = self
            .fetch_substate_from_committee(substate_id, specific_version)
            .await?;

        if let Some(version) = substate_result.version() {
            let should_update_cache = cached_version.is_none_or(|v| v < version);
            if should_update_cache {
                debug!(target: LOG_TARGET, "Updating cached substate {} with version {}", substate_id, version);
                let entry = SubstateCacheEntryRef {
                    version,
                    substate_result: &substate_result,
                    cached_at: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_secs(),
                };
                self.substate_cache.write(substate_id, entry).await?;
            }
        }

        Ok(substate_result)
    }

    pub async fn get_cached_substates<'a, I: Iterator<Item = &'a SubstateId> + ExactSizeIterator>(
        &self,
        substate_ids: I,
    ) -> Result<HashMap<&'a SubstateId, Option<SubstateCacheEntry>>, IndexerError> {
        let mut results = HashMap::with_capacity(substate_ids.len());
        for substate_id in substate_ids {
            let cache_res = self.substate_cache.read(substate_id).await?;
            results.insert(substate_id, cache_res);
        }
        Ok(results)
    }

    async fn build_vn_client_map<'a>(
        &self,
        substate_ids: &'a [SubstateId],
        epoch: Epoch,
        num_committees: u32,
    ) -> Result<HashMap<ShardGroup, (TVnClient::Client, Vec<&'a SubstateId>)>, IndexerError> {
        let mut client_map = HashMap::<_, (_, Vec<&'a SubstateId>)>::with_capacity(substate_ids.len());
        for substate_id in substate_ids {
            let shard_group = SubstateAddress::from_substate_id(substate_id, 0)
                .to_shard_group(NumPreshards::current(), num_committees);
            if let Some((_, substates_mut)) = client_map.get_mut(&shard_group) {
                substates_mut.push(substate_id);
                continue;
            }
            let member = self
                .committee_provider
                .get_random_committee_member(epoch, Some(shard_group), Default::default())
                .await?;
            let client = self.validator_node_client_factory.create_client(&member.address);
            client_map.insert(shard_group, (client, vec![substate_id]));
        }
        Ok(client_map)
    }

    pub async fn fetch_and_cache_substates(
        &self,
        substate_ids: &[SubstateId],
    ) -> Result<HashMap<SubstateId, Substate>, IndexerError> {
        let epoch = self.committee_provider.current_epoch().await?;
        let num_committees = self.committee_provider.get_num_committees(epoch).await?;
        let client_map = self.build_vn_client_map(substate_ids, epoch, num_committees).await?;

        let mut results = HashMap::with_capacity(substate_ids.len());
        for (shard_group, (mut client, substate_ids)) in client_map {
            debug!(target: LOG_TARGET, "Fetching {} substates from shard group {}", substate_ids.len(), shard_group);
            for batch in substate_ids.chunks(50) {
                let resp = client.get_substates_batch(batch).await.map_err(|e| {
                    IndexerError::ValidatorNodeClientError(format!(
                        "Failed to get substate batch for shard group {}: {}",
                        shard_group, e
                    ))
                })?;
                for (substate_id, substate) in &resp {
                    let substate_result = SubstateResult::Up {
                        substate: Box::new(substate.clone()),
                    };
                    let entry = SubstateCacheEntryRef {
                        version: substate.version(),
                        substate_result: &substate_result,
                        cached_at: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_secs(),
                    };
                    self.substate_cache.write(substate_id, entry).await?;
                }
                results.extend(resp);
            }
        }
        Ok(results)
    }

    async fn fetch_substate_from_committee(
        &self,
        substate_id: &SubstateId,
        specific_version: Option<u32>,
    ) -> Result<SubstateResult, IndexerError> {
        let requirement = SubstateRequirementRef::new(substate_id, specific_version);
        let substate_result = self.get_specific_substate_from_committee(requirement).await?;
        debug!(target: LOG_TARGET, "Substate result for {} with version {}: {:?}", substate_id, specific_version.display(), substate_result);
        Ok(substate_result)
    }

    /// Returns a specific version. If this is not found an error is returned.
    async fn get_specific_substate_from_committee(
        &self,
        substate_req: SubstateRequirementRef<'_>,
    ) -> Result<SubstateResult, IndexerError> {
        debug!(target: LOG_TARGET, "get_specific_substate_from_committee: {substate_req}");
        let epoch = self.committee_provider.current_epoch().await?;
        let committee = self
            .committee_provider
            .get_committee_for_substate(epoch, substate_req.or_zero_version().to_substate_address())
            .await?;
        if committee.is_empty() {
            return Err(IndexerError::NoCommitteeMembers {
                details: format!("No committee found for substate {} at epoch {}", substate_req, epoch),
            });
        }

        let f = (committee.len() - 1) / 3;
        let mut num_nexist_substate_results = 0;
        let mut last_error = None;
        for member in committee.shuffled() {
            let vn_addr = &member.address;
            debug!(target: LOG_TARGET, "Getting substate {} from vn {}", substate_req, vn_addr);

            match self.get_substate_from_vn(vn_addr, substate_req).await {
                Ok(substate_result) => {
                    debug!(target: LOG_TARGET, "Got substate result for {} from vn {}: {:?}", substate_req, vn_addr, substate_result);
                    match substate_result {
                        SubstateResult::Up { .. } | SubstateResult::Down { .. } => return Ok(substate_result),
                        SubstateResult::DoesNotExist => {
                            if num_nexist_substate_results > f {
                                return Ok(substate_result);
                            }
                            num_nexist_substate_results += 1;
                        },
                    }
                },
                Err(e) => {
                    // We ignore a single VN error and keep querying the rest of the committee
                    warn!(
                        target: LOG_TARGET,
                        "Could not get substate {} from vn {}: {}", substate_req, vn_addr, e
                    );
                    last_error = Some(e);
                },
            }
        }

        warn!(
            target: LOG_TARGET,
            "Could not get substate for shard {} from any of the validator nodes", substate_req,
        );

        if let Some(e) = last_error {
            return Err(e);
        }
        Ok(SubstateResult::DoesNotExist)
    }

    /// Gets a substate directly from querying a VN
    async fn get_substate_from_vn(
        &self,
        vn_addr: &TAddr,
        substate_requirement: SubstateRequirementRef<'_>,
    ) -> Result<SubstateResult, IndexerError> {
        // build a client with the VN
        let mut client = self.validator_node_client_factory.create_client(vn_addr);

        if !self.verify_substate_proofs {
            return client
                .get_substate(substate_requirement)
                .await
                .map_err(|e| IndexerError::ValidatorNodeClientError(e.to_string()));
        }

        let (result, proof) = client
            .get_substate_with_proof(substate_requirement)
            .await
            .map_err(|e| IndexerError::ValidatorNodeClientError(e.to_string()))?;

        // Verify up/down results against the committee. A missing or invalid proof disqualifies this
        // validator's response (fail-closed) so the caller tries another member. `DoesNotExist` is
        // not provable and is left to the existing f+1 agreement.
        match &result {
            SubstateResult::Up { substate } => {
                self.verify_substate_proof(
                    substate_requirement.substate_id(),
                    substate.version(),
                    Some(substate.substate_value()),
                    proof,
                )
                .await?;
            },
            SubstateResult::Down { version } => {
                self.verify_substate_proof(substate_requirement.substate_id(), *version, None, proof)
                    .await?;
            },
            SubstateResult::DoesNotExist => {},
        }

        Ok(result)
    }

    async fn verify_substate_proof(
        &self,
        substate_id: &SubstateId,
        version: u32,
        value: Option<&SubstateValue>,
        proof: Option<SubstateProofData>,
    ) -> Result<(), IndexerError> {
        let proof = proof.ok_or_else(|| IndexerError::SubstateProofVerificationFailed {
            details: format!("validator did not return a proof for {substate_id}"),
        })?;

        let commit_proof = CommittedBlockProof::from_bytes(&proof.commit_proof).map_err(|e| {
            IndexerError::SubstateProofVerificationFailed {
                details: format!("undecodable commit proof: {e}"),
            }
        })?;
        let shard_group = commit_proof
            .shard_group()
            .map_err(|e| IndexerError::SubstateProofVerificationFailed { details: e.to_string() })?;
        // The committee for the commit proof's epoch/shard group is what is expected to have signed it.
        let committee = self
            .committee_provider
            .get_committee_by_shard_group(commit_proof.epoch(), shard_group)
            .await?;

        verify_substate_value_proof(
            &commit_proof,
            &proof.substate_value_proof,
            substate_id,
            version,
            value,
            Epoch(proof.proof_epoch),
            committee.quorum_threshold(),
            |pk| Ok(committee.get_power_by_public_key(pk).unwrap_or_else(VotePower::zero)),
        )
        .map_err(|e| IndexerError::SubstateProofVerificationFailed { details: e.to_string() })?;

        Ok(())
    }
}
