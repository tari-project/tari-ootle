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
    sync::Arc,
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use log::*;
use tari_common_types::types::FixedHash;
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
use tari_ootle_storage::{
    consensus_models::{CommittedBlockProof, VerifiedBlockTip},
    verify_substate_value_proof,
    verify_substate_value_proof_against_root,
};
use tari_template_lib_types::constants::{PUBLIC_IDENTITY_RESOURCE_ADDRESS, STEALTH_TARI_RESOURCE_ADDRESS};
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

/// A store of committee-validated shard-group state merkle roots that the read path consults to
/// avoid re-validating a served commit proof's QC chain when its root is already trusted.
///
/// The trust decision is keyed on the 32-byte `state_merkle_root` scoped by `(epoch, shard_group)`:
/// a node cannot produce a substate value proof that verifies against a root a quorum already
/// signed, so reusing such a root is exactly as sound as re-validating the commit proof.
#[async_trait]
pub trait TrustedRootStore: std::fmt::Debug + Send + Sync + 'static {
    /// True if `root` is a recorded, committee-validated state merkle root for `(epoch, shard_group)`.
    async fn is_trusted(&self, epoch: Epoch, shard_group: ShardGroup, root: FixedHash) -> Result<bool, IndexerError>;

    /// Records a newly committee-validated tip so subsequent reads at this root hit the fast path.
    async fn record(&self, tip: VerifiedBlockTip) -> Result<(), IndexerError>;
}

/// Outcome of a substate lookup together with whether the value was committee-verified.
#[derive(Debug, Clone)]
pub struct SubstateLookupResult {
    pub result: SubstateResult,
    /// True when the value was proven against a committee-signed state root. False when proof
    /// verification is disabled, the result is `DoesNotExist` (not provable), or no committee member
    /// could supply a proof yet (e.g. nothing has been committed since an epoch change).
    pub verified: bool,
}

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
    /// When set, lets a read skip re-validating a served commit proof whose root is already trusted,
    /// and is warmed with newly-validated roots. See [`TrustedRootStore`].
    trusted_root_store: Option<Arc<dyn TrustedRootStore>>,
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
            trusted_root_store: None,
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

    /// Sets the trusted-root store used to skip commit-proof re-validation on a store hit (and warmed
    /// on a miss). Only meaningful together with [`Self::with_substate_proof_verification`].
    pub fn with_trusted_root_store(mut self, store: Arc<dyn TrustedRootStore>) -> Self {
        self.trusted_root_store = Some(store);
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
    ) -> Result<SubstateLookupResult, IndexerError> {
        debug!(target: LOG_TARGET, "get_substate: {}v{}", substate_id, specific_version.display());
        let mut cached_version = None;
        // start from the latest cached version of the substate (if cached previously)
        let cache_res = self.substate_cache.read(substate_id).await?;
        if let Some(entry) = cache_res {
            // An unverified entry (e.g. written by the batch path or before verification was
            // enabled) is never served while verification is on: refetch so it can be replaced with
            // a proven copy.
            if entry.verified || !self.verify_substate_proofs {
                if let Some(version) = specific_version {
                    if entry.version == version {
                        debug!(target: LOG_TARGET, "Substate cache hit for {} with version {}", entry.version, substate_id);
                        #[cfg(feature = "metrics")]
                        self.metrics.as_ref().inspect(|m| m.inc_cache_hits());
                        return Ok(SubstateLookupResult {
                            result: entry.substate_result,
                            verified: entry.verified,
                        });
                    }
                } else {
                    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_secs();
                    let is_stale = now.saturating_sub(entry.cached_at) > self.cache_ttl.as_secs();
                    // A cached `Down` result means the substate has advanced past `entry.version`. An
                    // unversioned lookup asks for the *latest* version, so a `Down` entry can never
                    // satisfy it: refetch from the committee (which always returns the latest Up for an
                    // unversioned request) to discover the new version. Serving the stale `Down`
                    // surfaces as a spurious "input substate is down" error — e.g. dry-running a
                    // transaction whose unversioned inputs include a frequently-updated substate such as
                    // a swap pool reserve.
                    let is_down = matches!(entry.substate_result, SubstateResult::Down { .. });
                    if is_stale {
                        debug!(target: LOG_TARGET, "Cached substate {} is stale. Fetching fresh copy.", substate_id);
                    } else if is_down {
                        debug!(target: LOG_TARGET, "Cached substate {} is down at v{}. Fetching latest version.", substate_id, entry.version);
                    } else {
                        debug!(target: LOG_TARGET, "Substate cache hit for {} with version {}", substate_id, entry.version);
                        #[cfg(feature = "metrics")]
                        self.metrics.as_ref().inspect(|m| m.inc_cache_hits());
                        return Ok(SubstateLookupResult {
                            result: entry.substate_result.clone(),
                            verified: entry.verified,
                        });
                    }
                }

                cached_version = Some(entry.version);
            }
        }
        #[cfg(feature = "metrics")]
        self.metrics.as_ref().inspect(|m| m.inc_cache_misses());

        let lookup_result = self
            .fetch_substate_from_committee(substate_id, specific_version)
            .await?;

        if let Some(version) = lookup_result.result.version() {
            // Unverified results are not cached while verification is on, so the next read retries
            // for a proven copy instead of pinning the unverified value for the TTL.
            let should_update_cache =
                (lookup_result.verified || !self.verify_substate_proofs) && cached_version.is_none_or(|v| v < version);
            if should_update_cache {
                debug!(target: LOG_TARGET, "Updating cached substate {} with version {}", substate_id, version);
                let entry = SubstateCacheEntryRef {
                    version,
                    substate_result: &lookup_result.result,
                    cached_at: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_secs(),
                    verified: lookup_result.verified,
                };
                self.substate_cache.write(substate_id, entry).await?;
            }
        }

        Ok(lookup_result)
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
                    // The batch RPC does not carry proofs, so these entries are always unverified.
                    let entry = SubstateCacheEntryRef {
                        version: substate.version(),
                        substate_result: &substate_result,
                        cached_at: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_secs(),
                        verified: false,
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
    ) -> Result<SubstateLookupResult, IndexerError> {
        let requirement = SubstateRequirementRef::new(substate_id, specific_version);
        let lookup_result = self.get_specific_substate_from_committee(requirement).await?;
        debug!(target: LOG_TARGET, "Substate result for {} with version {}: {:?}", substate_id, specific_version.display(), lookup_result);
        Ok(lookup_result)
    }

    /// Returns a specific version. If this is not found an error is returned.
    async fn get_specific_substate_from_committee(
        &self,
        substate_req: SubstateRequirementRef<'_>,
    ) -> Result<SubstateLookupResult, IndexerError> {
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
        // Highest-version Up/Down response that came back without a proof. Only served if no member
        // can prove.
        let mut unproven_result: Option<SubstateResult> = None;
        for member in committee.shuffled() {
            let vn_addr = &member.address;
            debug!(target: LOG_TARGET, "Getting substate {} from vn {}", substate_req, vn_addr);

            match self.get_substate_from_vn(vn_addr, substate_req).await {
                Ok((substate_result, verified)) => {
                    debug!(target: LOG_TARGET, "Got substate result for {} from vn {} (verified = {}): {:?}", substate_req, vn_addr, verified, substate_result);
                    match substate_result {
                        SubstateResult::Up { .. } | SubstateResult::Down { .. } => {
                            if verified || !self.verify_substate_proofs {
                                return Ok(SubstateLookupResult {
                                    result: substate_result,
                                    verified,
                                });
                            }
                            // The member could not prove its response (e.g. nothing committed since
                            // the epoch started). Keep the highest version as a fallback (a member
                            // that is still syncing may respond with a stale copy) and try the rest
                            // of the committee for a proven copy.
                            if unproven_result
                                .as_ref()
                                .is_none_or(|r| r.version() < substate_result.version())
                            {
                                unproven_result = Some(substate_result);
                            }
                        },
                        SubstateResult::DoesNotExist => {
                            if num_nexist_substate_results > f {
                                return Ok(SubstateLookupResult {
                                    result: substate_result,
                                    verified: false,
                                });
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

        if let Some(result) = unproven_result {
            warn!(
                target: LOG_TARGET,
                "No committee member could supply a proof for {substate_req}. Returning the substate unverified.",
            );
            return Ok(SubstateLookupResult {
                result,
                verified: false,
            });
        }

        warn!(
            target: LOG_TARGET,
            "Could not get substate for shard {} from any of the validator nodes", substate_req,
        );

        if let Some(e) = last_error {
            return Err(e);
        }
        Ok(SubstateLookupResult {
            result: SubstateResult::DoesNotExist,
            verified: false,
        })
    }

    /// Gets a substate directly from querying a VN. The returned flag is true if the result came
    /// with a proof that verified against the committee.
    async fn get_substate_from_vn(
        &self,
        vn_addr: &TAddr,
        substate_requirement: SubstateRequirementRef<'_>,
    ) -> Result<(SubstateResult, bool), IndexerError> {
        // build a client with the VN
        let mut client = self.validator_node_client_factory.create_client(vn_addr);

        if !self.verify_substate_proofs {
            return client
                .get_substate(substate_requirement)
                .await
                .map(|result| (result, false))
                .map_err(|e| IndexerError::ValidatorNodeClientError(e.to_string()));
        }

        // The TARI resource and the public identity resource are immutable protocol constants
        // bootstrapped directly into the substate store at genesis. They are never written to the
        // shard state tree, so no inclusion proof can be produced for them and verification would
        // always fail with a leaf-key mismatch. Their value is fixed by the protocol, so they are
        // verified by definition: fetch without a proof and treat as verified.
        //
        // This is deliberately restricted to these two genesis addresses. Other read-only substates
        // (templates, transaction receipts, claimed output tombstones) are created during
        // transaction execution and *are* committed to the state tree, so they must still be proven.
        //
        // TODO: commit genesis substates to the state tree so they can be cryptographically verified
        //       like any other substate (changes the genesis state root, so deferred).
        let resource_address = substate_requirement.substate_id().as_resource_address();
        if resource_address == Some(STEALTH_TARI_RESOURCE_ADDRESS) ||
            resource_address == Some(PUBLIC_IDENTITY_RESOURCE_ADDRESS)
        {
            return client
                .get_substate(substate_requirement)
                .await
                .map(|result| (result, true))
                .map_err(|e| IndexerError::ValidatorNodeClientError(e.to_string()));
        }

        let (result, proof) = client
            .get_substate_with_proof(substate_requirement)
            .await
            .map_err(|e| IndexerError::ValidatorNodeClientError(e.to_string()))?;

        // The validator has nothing committed to anchor a proof against yet (e.g. immediately after
        // an epoch change). Return the result unverified and let the caller decide.
        let Some(proof) = proof else {
            return Ok((result, false));
        };

        // Verify up/down results against the committee. An invalid proof disqualifies this
        // validator's response (fail-closed) so the caller tries another member. `DoesNotExist` is
        // not provable and is left to the existing f+1 agreement.
        let verified = match &result {
            SubstateResult::Up { substate } => {
                self.verify_substate_proof(
                    substate_requirement.substate_id(),
                    substate.version(),
                    Some(substate.substate_value()),
                    proof,
                )
                .await?;
                true
            },
            SubstateResult::Down { version } => {
                self.verify_substate_proof(substate_requirement.substate_id(), *version, None, proof)
                    .await?;
                true
            },
            SubstateResult::DoesNotExist => false,
        };

        Ok((result, verified))
    }

    async fn verify_substate_proof(
        &self,
        substate_id: &SubstateId,
        version: u32,
        value: Option<&SubstateValue>,
        proof: SubstateProofData,
    ) -> Result<(), IndexerError> {
        let commit_proof = CommittedBlockProof::from_bytes(&proof.commit_proof).map_err(|e| {
            IndexerError::SubstateProofVerificationFailed {
                details: format!("undecodable commit proof: {e}"),
            }
        })?;
        let epoch = commit_proof.epoch();
        let shard_group = commit_proof
            .shard_group()
            .map_err(|e| IndexerError::SubstateProofVerificationFailed { details: e.to_string() })?;
        let root = commit_proof.state_merkle_root();

        // Fast path: if this exact (epoch, shard_group, root) was already committee-validated and
        // recorded in the trusted-root store, verify the value proof directly against the trusted
        // root and skip re-validating the commit proof's QC chain (and the committee lookup). A node
        // cannot forge a value proof that verifies against a root a quorum already signed, so this is
        // as sound as the full path.
        if let Some(store) = &self.trusted_root_store &&
            store.is_trusted(epoch, shard_group, root).await?
        {
            verify_substate_value_proof_against_root(
                &proof.substate_value_proof,
                substate_id,
                version,
                value,
                Epoch(proof.proof_epoch),
                root,
            )
            .map_err(|e| IndexerError::SubstateProofVerificationFailed { details: e.to_string() })?;
            debug!(
                target: LOG_TARGET,
                "trusted-root HIT for {substate_id} at epoch {epoch} {shard_group}: skipped commit-proof validation"
            );
            return Ok(());
        }

        // Slow path: validate the commit proof against the shard group committee, yielding a trusted
        // root, then verify the value proof against it.
        let committee = self
            .committee_provider
            .get_committee_by_shard_group(epoch, shard_group)
            .await?;

        let verified_tip = verify_substate_value_proof(
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
        debug!(
            target: LOG_TARGET,
            "trusted-root MISS for {substate_id} at epoch {epoch} {shard_group}: validated commit proof"
        );

        // Warm the store so subsequent reads at this tip hit the fast path. A write failure must not
        // fail an otherwise-verified read.
        if let Some(store) = &self.trusted_root_store &&
            let Err(e) = store.record(verified_tip).await
        {
            warn!(target: LOG_TARGET, "Failed to record verified root for {substate_id}: {e}");
        }

        Ok(())
    }
}
