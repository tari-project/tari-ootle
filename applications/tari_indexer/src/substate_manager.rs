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
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use ootle_network::Network;
use serde::{Deserialize, Serialize};
use tari_engine_types::{
    Utxo,
    encoded::Encoded,
    substate::{EncodedSubstate, Substate, SubstateId, SubstateValue},
};
use tari_epoch_manager::service::EpochManagerHandle;
use tari_indexer_lib::{
    cached_substate_manager::{
        CachedSubstateManager,
        InputSubstatesLookup,
        ProvenSubstate,
        SubstateLookupResult,
        TrustedRootStore,
    },
    error::IndexerError,
};
use tari_ootle_common_types::{
    Epoch,
    NodeHeight,
    NumPreshards,
    ShardGroup,
    StateVersion,
    SubstateRequirementRef,
    SubstateVersion,
    optional::{IsNotFoundError, Optional},
    shard::Shard,
};
use tari_ootle_p2p::PeerAddress;
use tari_ootle_storage::{StorageError, consensus_models::VerifiedBlockTip};
use tari_sidechain::FixedHash;
use tari_template_lib_types::{
    ResourceAddress,
    UtxoId,
    crypto::{RistrettoPublicKeyBytes, UtxoTag},
};
use tari_validator_node_rpc::client::{SubstateProofData, SubstateResult, TariValidatorNodeRpcClientFactory};

use crate::{
    network_state_sync::SyncProgress,
    storage_sqlite::{
        SqliteIndexerStore,
        models::{Key, VerifiedStateRoot},
    },
    store::{IndexerStore, IndexerStoreReadTransaction, IndexerStoreReader, IndexerStoreWriteTransaction},
    substate_cache::SqliteSubstateCache,
};

/// A page of the unspent UTXOs matching a lookup's tag and nonce pairs, and the pairs that are spent or burnt.
#[derive(Debug)]
pub struct FetchedUtxos {
    pub utxos: Vec<(UtxoId, Encoded<Utxo>)>,
    pub spent: Vec<(UtxoTag, RistrettoPublicKeyBytes)>,
    /// The UTXO after which more unspent UTXOs match, or `None` once `utxos` holds the last of them.
    pub next_cursor: Option<UtxoId>,
}

/// Up to `limit` of the unspent UTXOs matching `public_nonce_and_tag` in id order, after `cursor` when given,
/// and the pairs of `public_nonce_and_tag` that are spent or burnt.
///
/// `unspent_at` is empty, or holds for each pair of `public_nonce_and_tag` the shard and state version its UTXO was
/// unspent at, if known. A pair with no unspent UTXO is spent when its shard is promoted through that state version,
/// whether or not its tombstone is kept: promotion writes every UTXO of a version with the shard's verified version, in
/// one transaction, and an unspent UTXO is never pruned.
fn read_utxos<TTx: IndexerStoreReadTransaction>(
    tx: &mut TTx,
    resource_address: &ResourceAddress,
    public_nonce_and_tag: &[(UtxoTag, RistrettoPublicKeyBytes)],
    unspent_at: &[Option<(Shard, StateVersion)>],
    cursor: Option<UtxoId>,
    limit: u32,
) -> Result<FetchedUtxos, StorageError> {
    let mut utxos =
        tx.utxos_get_unspent_by_public_nonce_and_tag(resource_address, public_nonce_and_tag, cursor, limit + 1)?;
    let next_cursor = if utxos.len() > limit as usize {
        utxos.truncate(limit as usize);
        utxos.last().map(|(id, _)| *id)
    } else {
        None
    };
    let mut spent = tx.utxos_get_spent_public_nonce_and_tag(resource_address, public_nonce_and_tag)?;
    if unspent_at.iter().all(Option::is_none) {
        return Ok(FetchedUtxos {
            utxos,
            spent,
            next_cursor,
        });
    }

    let progress = tx
        .key_value_get_value::<_, SyncProgress>(Key::SyncProgress)
        .optional()?
        .unwrap_or_default();
    let reported = spent.iter().copied().collect::<HashSet<_>>();
    let promoted = public_nonce_and_tag
        .iter()
        .zip(unspent_at)
        .filter(|(pair, unspent_at)| {
            !reported.contains(*pair) &&
                unspent_at.is_some_and(|(shard, state_version)| {
                    // A shard nothing is promoted of has no entry
                    progress
                        .verified
                        .get(&shard)
                        .is_some_and(|verified| verified.state_version >= state_version)
                })
        })
        .map(|(pair, _)| *pair)
        .collect::<Vec<_>>();
    if promoted.is_empty() {
        return Ok(FetchedUtxos {
            utxos,
            spent,
            next_cursor,
        });
    }
    let held = tx
        .utxos_get_unspent_public_nonce_and_tag(resource_address, &promoted)?
        .into_iter()
        .collect::<HashSet<_>>();
    spent.extend(promoted.into_iter().filter(|pair| !held.contains(pair)));
    Ok(FetchedUtxos {
        utxos,
        spent,
        next_cursor,
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubstateResponse {
    pub id: SubstateId,
    pub version: SubstateVersion,
    pub substate: Encoded<SubstateValue>,
}

/// A substate returned by a lookup together with whether it was committee-verified. `verified` is
/// false when proof verification is disabled, or when no committee member could supply a proof yet
/// (e.g. nothing has been committed since an epoch change).
#[derive(Debug, Clone)]
pub struct FetchedSubstate {
    pub substate: EncodedSubstate,
    pub verified: bool,
    /// The proof `substate` was verified with, when it was fetched from the committee and proven.
    pub proof: Option<SubstateProofData>,
}

/// What a lookup with proofs found for a substate that exists.
#[derive(Debug, Clone)]
pub enum ProvenLookup {
    Up(Box<FetchedSubstate>),
    /// The version the lookup named is down. A verified `substate_down_proof` proves it. Without one, `proof` is an
    /// exclusion verified at its anchor, and more than `f` members agreed the version is down.
    Down {
        version: SubstateVersion,
        proof: SubstateProofData,
    },
}

/// Adapts the indexer's sqlite store to [`TrustedRootStore`], which the read path consults to skip
/// commit-proof re-validation on a trusted-root hit and warms with newly-validated roots.
#[derive(Clone)]
struct SqliteTrustedRootStore {
    store: SqliteIndexerStore,
}

impl std::fmt::Debug for SqliteTrustedRootStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteTrustedRootStore").finish_non_exhaustive()
    }
}

#[async_trait]
impl TrustedRootStore for SqliteTrustedRootStore {
    async fn trusted_root_height(
        &self,
        epoch: Epoch,
        shard_group: ShardGroup,
        root: FixedHash,
    ) -> Result<Option<NodeHeight>, IndexerError> {
        self.store
            .with_read_tx(move |tx| tx.get_verified_state_root_height(epoch, shard_group, &root))
            .await
            .map_err(|e: StorageError| IndexerError::TrustedRootStoreError(e.to_string()))
    }

    async fn latest_height(&self, epoch: Epoch, shard_group: ShardGroup) -> Result<Option<NodeHeight>, IndexerError> {
        self.store
            .with_read_tx(move |tx| tx.get_latest_verified_height_overlapping(epoch, shard_group))
            .await
            .map_err(|e: StorageError| IndexerError::TrustedRootStoreError(e.to_string()))
    }

    async fn record(&self, tip: VerifiedBlockTip) -> Result<(), IndexerError> {
        let root = VerifiedStateRoot::from_verified_tip(&tip);
        self.store
            .with_write_tx(move |tx| tx.upsert_verified_state_root(&root))
            .await
            .map_err(|e: StorageError| IndexerError::TrustedRootStoreError(e.to_string()))
    }
}

#[derive(Debug, Clone)]
pub struct SubstateManager {
    cache_manager:
        CachedSubstateManager<EpochManagerHandle<PeerAddress>, TariValidatorNodeRpcClientFactory, SqliteSubstateCache>,
    substate_store: SqliteIndexerStore,
}

impl SubstateManager {
    pub fn new(
        network: Network,
        num_preshards: NumPreshards,
        substate_store: SqliteIndexerStore,
        epoch_manager: EpochManagerHandle<PeerAddress>,
        validator_node_client_factory: TariValidatorNodeRpcClientFactory,
        substate_cache: SqliteSubstateCache,
    ) -> Self {
        // Consulted only when proof verification is enabled (the dry-run manager has it off, so its
        // copy is inert). Lets a read skip re-validating a served commit proof when its root is
        // already trusted, and is warmed with newly-validated roots.
        let trusted_root_store = Arc::new(SqliteTrustedRootStore {
            store: substate_store.clone(),
        });
        let cached_substates = CachedSubstateManager::new(
            network,
            num_preshards,
            epoch_manager.clone(),
            validator_node_client_factory.clone(),
            substate_cache,
        )
        .with_trusted_root_store(trusted_root_store);

        Self {
            cache_manager: cached_substates,
            substate_store,
        }
    }

    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_manager = self.cache_manager.with_cache_ttl(ttl);
        self
    }

    pub fn with_negative_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_manager = self.cache_manager.with_negative_cache_ttl(ttl);
        self
    }

    pub fn with_substate_proof_verification(mut self, enabled: bool) -> Self {
        self.cache_manager = self.cache_manager.with_substate_proof_verification(enabled);
        self
    }

    /// See [`CachedSubstateManager::with_cache_writes`].
    pub fn with_cache_writes(mut self, enabled: bool) -> Self {
        self.cache_manager = self.cache_manager.with_cache_writes(enabled);
        self
    }

    /// Whether substates served by this manager are verified against the shard group committee.
    pub fn verifies_substates(&self) -> bool {
        self.cache_manager.verifies_substates()
    }

    #[cfg(feature = "metrics")]
    pub fn with_metrics(self, registry_mut: &mut prometheus_client::registry::Registry) -> Self {
        let cached_substates = self.cache_manager.with_metrics(registry_mut);
        Self {
            cache_manager: cached_substates,
            substate_store: self.substate_store,
        }
    }

    /// A page of the unspent UTXOs matching `public_nonce_and_tag`, and the pairs of `public_nonce_and_tag` that are
    /// spent or burnt, read in one transaction. See [`read_utxos`].
    pub async fn fetch_utxos(
        &self,
        resource_address: &ResourceAddress,
        public_nonce_and_tag: &[(UtxoTag, RistrettoPublicKeyBytes)],
        unspent_at: &[Option<(Shard, StateVersion)>],
        cursor: Option<UtxoId>,
        limit: u32,
    ) -> Result<FetchedUtxos, SubstateManagerError> {
        let resource_address = *resource_address;
        let public_nonce_and_tag = public_nonce_and_tag.to_vec();
        let unspent_at = unspent_at.to_vec();
        let utxos = self
            .substate_store
            .with_read_tx(move |tx| {
                read_utxos(tx, &resource_address, &public_nonce_and_tag, &unspent_at, cursor, limit)
            })
            .await?;
        Ok(utxos)
    }

    pub async fn get_substate(&self, req: SubstateRequirementRef<'_>) -> Result<Substate, SubstateManagerError> {
        let lookup_result = self
            .cache_manager
            .get_substate(req.substate_id(), req.version())
            .await?;
        match lookup_result.result {
            SubstateResult::Up { substate } => Ok(decode_substate(req.substate_id(), &substate)?),
            SubstateResult::Down { version } => Err(SubstateManagerError::InputSubstateIsDown {
                substate_id: req.substate_id().clone(),
                version,
            }),
            SubstateResult::DoesNotExist => Err(SubstateManagerError::InputSubstateDoesNotExist {
                substate_id: req.substate_id().clone(),
            }),
        }
    }

    /// Looks up one substate. `None` when the committee agrees it does not exist.
    pub async fn fetch_substate(
        &self,
        req: SubstateRequirementRef<'_>,
    ) -> Result<Option<FetchedSubstate>, SubstateManagerError> {
        if let Some(fetched) = self.get_versioned_substate_from_db(req).await? {
            return Ok(Some(fetched));
        }
        let lookup_result = self
            .cache_manager
            .get_substate(req.substate_id(), req.version())
            .await?;
        Self::to_fetched_substate(req, lookup_result)
    }

    /// Looks up one substate from the committee, with the proof it was verified with. `None` when the
    /// committee agrees it does not exist.
    ///
    /// A down version is answered with the proof the indexer verified for it, a down proof or an exclusion, which only
    /// a lookup naming that version can return; any other down is [`SubstateManagerError::InputSubstateIsDown`].
    ///
    /// The local substate store holds no proofs, so it is not consulted. A cached head is served with
    /// the proof held for it, and anything else is fetched from the committee.
    pub async fn fetch_substate_with_proof(
        &self,
        req: SubstateRequirementRef<'_>,
    ) -> Result<Option<ProvenLookup>, SubstateManagerError> {
        let lookup_result = self
            .cache_manager
            .get_substate_with_proof(req.substate_id(), req.version())
            .await?;
        Self::to_proven_lookup(req, lookup_result)
    }

    fn to_proven_lookup(
        req: SubstateRequirementRef<'_>,
        lookup_result: SubstateLookupResult,
    ) -> Result<Option<ProvenLookup>, SubstateManagerError> {
        match lookup_result {
            SubstateLookupResult {
                result: SubstateResult::Down { version },
                proof: Some(proof),
                ..
            } => Ok(Some(ProvenLookup::Down { version, proof })),
            lookup_result => {
                Ok(Self::to_fetched_substate(req, lookup_result)?.map(|fetched| ProvenLookup::Up(Box::new(fetched))))
            },
        }
    }

    fn to_fetched_substate(
        req: SubstateRequirementRef<'_>,
        lookup_result: SubstateLookupResult,
    ) -> Result<Option<FetchedSubstate>, SubstateManagerError> {
        match lookup_result.result {
            SubstateResult::DoesNotExist => Ok(None),
            SubstateResult::Up { substate } => Ok(Some(FetchedSubstate {
                substate,
                verified: lookup_result.verified,
                proof: lookup_result.proof,
            })),
            SubstateResult::Down { version } => Err(SubstateManagerError::InputSubstateIsDown {
                substate_id: req.substate_id().clone(),
                version,
            }),
        }
    }

    /// Looks up the substates a transaction declares as inputs. Fails on the first one that is not
    /// up, as consensus would.
    pub async fn get_input_substates<'a, I: IntoIterator<Item = SubstateRequirementRef<'a>>>(
        &self,
        substate_req: I,
    ) -> Result<HashMap<SubstateId, FetchedSubstate>, SubstateManagerError> {
        let substate_req = substate_req.into_iter().collect::<HashSet<_>>();
        let mut results = HashMap::with_capacity(substate_req.len());

        let mut remaining = Vec::with_capacity(substate_req.len());
        for req in substate_req {
            match self.get_versioned_substate_from_db(req).await? {
                Some(fetched) => {
                    results.insert(req.substate_id().clone(), fetched);
                },
                None => remaining.push(req),
            }
        }

        match self.cache_manager.get_input_substates(&remaining).await? {
            InputSubstatesLookup::AllUp(found) => {
                for (id, lookup_result) in found {
                    let Some(substate) = lookup_result.result.into_up() else {
                        continue;
                    };
                    results.insert(id, FetchedSubstate {
                        substate,
                        verified: lookup_result.verified,
                        proof: lookup_result.proof,
                    });
                }
                Ok(results)
            },
            InputSubstatesLookup::Down { substate_id, version } => {
                Err(SubstateManagerError::InputSubstateIsDown { substate_id, version })
            },
            InputSubstatesLookup::DoesNotExist { substate_id } => {
                Err(SubstateManagerError::InputSubstateDoesNotExist { substate_id })
            },
        }
    }

    /// A version-pinned requirement served from the local store, if held there.
    async fn get_versioned_substate_from_db(
        &self,
        req: SubstateRequirementRef<'_>,
    ) -> Result<Option<FetchedSubstate>, SubstateManagerError> {
        let Some(version) = req.version() else {
            return Ok(None);
        };
        let Some(substate) = self.get_substate_from_db(req.substate_id(), Some(version)).await? else {
            return Ok(None);
        };
        Ok(Some(FetchedSubstate {
            substate: EncodedSubstate::new(substate.version, substate.substate),
            // Locally-stored substates were verified when ingested iff verification is on.
            verified: self.cache_manager.verifies_substates(),
            proof: None,
        }))
    }

    pub async fn get_cached_substates(
        &self,
        substates: &[SubstateId],
    ) -> Result<HashMap<SubstateId, EncodedSubstate>, SubstateManagerError> {
        let mut substate_set = substates.iter().collect::<HashSet<_>>();

        let substates_arg = substates.to_vec();
        let mut substates = self
            .substate_store
            .with_read_tx(move |tx| tx.get_substates(&substates_arg))
            .await?;

        for k in substates.keys() {
            substate_set.remove(k);
        }

        let cached = self
            .cache_manager
            .get_cached_substates(substate_set.into_iter())
            .await?;
        for (id, substate) in cached {
            let Some(substate) = substate else {
                continue;
            };
            let Some(substate) = substate.substate_result.into_up() else {
                continue;
            };
            substates.insert(id.clone(), substate);
        }

        Ok(substates)
    }

    pub async fn fetch_and_cache_substates(
        &self,
        substate_ids: &[SubstateId],
    ) -> Result<HashMap<SubstateId, ProvenSubstate>, SubstateManagerError> {
        let result = self.cache_manager.fetch_and_cache_substates(substate_ids).await?;
        Ok(result)
    }

    async fn get_substate_from_db(
        &self,
        substate_address: &SubstateId,
        version: Option<SubstateVersion>,
    ) -> Result<Option<SubstateResponse>, SubstateManagerError> {
        let substate_address = substate_address.clone();
        let row = self
            .substate_store
            .with_read_tx(move |tx| tx.get_substate(&substate_address, version))
            .await?;
        match row {
            Some(row) => Ok(Some(row.try_into()?)),
            None => Ok(None),
        }
    }
}

pub fn decode_substate(substate_id: &SubstateId, substate: &EncodedSubstate) -> Result<Substate, SubstateManagerError> {
    substate
        .decode()
        .map_err(|e| SubstateManagerError::UndecodableSubstate {
            substate_id: substate_id.clone(),
            version: substate.version(),
            details: e.to_string(),
        })
}

#[derive(thiserror::Error, Debug)]
pub enum SubstateManagerError {
    #[error("Indexer error: {0}")]
    IndexerError(#[from] IndexerError),
    #[error("Storage error: {0}")]
    StorageError(#[from] StorageError),
    #[error("Input substate {substate_id} (v{version}) is down")]
    InputSubstateIsDown {
        substate_id: SubstateId,
        version: SubstateVersion,
    },
    #[error("Input substate {substate_id} does not exist")]
    InputSubstateDoesNotExist { substate_id: SubstateId },
    #[error("The value served for {substate_id} v{version} does not decode: {details}")]
    UndecodableSubstate {
        substate_id: SubstateId,
        version: SubstateVersion,
        details: String,
    },
}

impl IsNotFoundError for SubstateManagerError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::InputSubstateDoesNotExist { .. }) ||
            matches!(self, Self::StorageError(e) if e.is_not_found_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn substate() -> SubstateId {
        format!("component_{:064x}", 1).parse().unwrap()
    }

    fn down(proof: Option<SubstateProofData>) -> SubstateLookupResult {
        SubstateLookupResult {
            result: SubstateResult::Down {
                version: SubstateVersion::new(3),
            },
            verified: proof.is_some(),
            proof,
        }
    }

    fn proof_data(substate_down_proof: Option<Vec<u8>>) -> SubstateProofData {
        SubstateProofData {
            substate_value_proof: vec![1],
            commit_proof: vec![2],
            proof_epoch: 0,
            substate_down_proof,
            destroyed_at_state_version: Some(4),
        }
    }

    #[test]
    fn a_proven_down_is_answered_with_its_proof() {
        let id = substate();
        let lookup = SubstateManager::to_proven_lookup(
            SubstateRequirementRef::new(&id, Some(SubstateVersion::new(3))),
            down(Some(proof_data(Some(vec![7])))),
        )
        .unwrap();

        let Some(ProvenLookup::Down { version, proof }) = lookup else {
            panic!("expected a proven down, got {lookup:?}");
        };
        assert_eq!(version, SubstateVersion::new(3));
        assert_eq!(proof.substate_down_proof, Some(vec![7]));
    }

    #[test]
    fn a_down_settled_with_an_exclusion_is_answered_with_it() {
        let id = substate();
        let lookup_result = SubstateLookupResult {
            verified: false,
            ..down(Some(proof_data(None)))
        };
        let lookup = SubstateManager::to_proven_lookup(
            SubstateRequirementRef::new(&id, Some(SubstateVersion::new(3))),
            lookup_result,
        )
        .unwrap();

        let Some(ProvenLookup::Down { version, proof }) = lookup else {
            panic!("expected a proven down, got {lookup:?}");
        };
        assert_eq!(version, SubstateVersion::new(3));
        assert_eq!(proof.substate_down_proof, None);
    }

    fn pair(seq: u8) -> (UtxoTag, RistrettoPublicKeyBytes) {
        (
            UtxoTag::from(0u32),
            RistrettoPublicKeyBytes::from_bytes(&[seq; 32]).unwrap(),
        )
    }

    #[tokio::test]
    async fn a_placed_pair_with_no_unspent_utxo_is_spent_once_its_shard_is_promoted_through_its_version() {
        use tari_state_tree::TransitionAccumulator;

        use crate::network_state_sync::VerifiedShardVersion;

        let (_dir, store) = crate::storage_sqlite::temp_store().await;
        let mut progress = SyncProgress::default();
        progress.record_verified(Shard::from(1u32), VerifiedShardVersion {
            state_version: StateVersion::new(10),
            epoch: Epoch(1),
            transition_acc: TransitionAccumulator::zero(),
        });
        store
            .with_write_tx(move |tx| {
                tx.batch_insert_utxo_updates(Epoch(1), [crate::storage_sqlite::unspent_at(1, 5)])?;
                tx.key_value_set(Key::SyncProgress, progress)
            })
            .await
            .unwrap();

        let pairs = [pair(1), pair(2), pair(3), pair(4), pair(5), pair(6)];
        let unspent_at = [
            Some((Shard::from(1u32), StateVersion::new(5))),
            Some((Shard::from(1u32), StateVersion::new(10))),
            Some((Shard::from(1u32), StateVersion::new(11))),
            None,
            Some((Shard::from(2u32), StateVersion::new(1))),
            Some((Shard::from(1u32), StateVersion::new(1))),
        ];
        let fetched = store
            .with_read_tx(move |tx| {
                read_utxos(
                    tx,
                    &crate::storage_sqlite::utxo_resource(),
                    &pairs,
                    &unspent_at,
                    None,
                    10,
                )
            })
            .await
            .unwrap();

        assert_eq!(fetched.utxos.len(), 1, "the unspent UTXO is returned");
        assert_eq!(
            fetched.spent,
            [pair(2), pair(6)],
            "only absent pairs promoted through are spent"
        );

        let fetched = store
            .with_read_tx(move |tx| read_utxos(tx, &crate::storage_sqlite::utxo_resource(), &pairs, &[], None, 10))
            .await
            .unwrap();
        assert!(
            fetched.spent.is_empty(),
            "a pair without a placement is not known to be spent"
        );
    }

    #[tokio::test]
    async fn every_unspent_utxo_sharing_a_pair_is_fetched_a_page_at_a_time() {
        use tari_template_lib_types::{UtxoAddress, UtxoId};

        use crate::storage_sqlite::models::UtxoUpdateRecord;

        let (_dir, store) = crate::storage_sqlite::temp_store().await;
        let copies = [2u8, 3].map(|seq| {
            let UtxoUpdateRecord::Unspent(mut copy) = crate::storage_sqlite::unspent_at(1, 5) else {
                unreachable!()
            };
            copy.address = UtxoAddress::new(crate::storage_sqlite::utxo_resource(), UtxoId::from_array([seq; 32]));
            UtxoUpdateRecord::Unspent(copy)
        });
        let skipped = store
            .with_write_tx(move |tx| {
                tx.batch_insert_utxo_updates(
                    Epoch(1),
                    [crate::storage_sqlite::unspent_at(1, 5)].into_iter().chain(copies),
                )
            })
            .await
            .unwrap();
        assert!(skipped.is_empty(), "UTXOs sharing a nonce and tag are all indexed");

        let page = |cursor: Option<UtxoId>| {
            let store = store.clone();
            async move {
                store
                    .with_read_tx(move |tx| {
                        read_utxos(tx, &crate::storage_sqlite::utxo_resource(), &[pair(1)], &[], cursor, 2)
                    })
                    .await
                    .unwrap()
            }
        };
        let first = page(None).await;
        assert!(first.next_cursor.is_some());
        let ids = first.utxos.iter().map(|(id, _)| *id).collect::<Vec<_>>();
        assert_eq!(ids, [UtxoId::from_array([1; 32]), UtxoId::from_array([2; 32])]);

        // A re-version of the last UTXO served stores a later row, which the cursor does not skip to
        store
            .with_write_tx(move |tx| {
                let UtxoUpdateRecord::Unspent(mut refrozen) = crate::storage_sqlite::unspent_at(2, 6) else {
                    unreachable!()
                };
                refrozen.address =
                    UtxoAddress::new(crate::storage_sqlite::utxo_resource(), UtxoId::from_array([2; 32]));
                refrozen.version = SubstateVersion::new(1);
                refrozen.utxo_output.output.public_nonce = pair(1).1;
                tx.batch_insert_utxo_updates(Epoch(1), [
                    crate::storage_sqlite::spent_at(2, 6),
                    UtxoUpdateRecord::Unspent(refrozen),
                ])
            })
            .await
            .unwrap();

        let second = page(first.next_cursor).await;
        assert_eq!(second.next_cursor, None);
        let ids = second.utxos.iter().map(|(id, _)| *id).collect::<Vec<_>>();
        assert_eq!(ids, [UtxoId::from_array([3; 32])]);
    }

    /// Indexers behind one address store the same outputs in different orders and at different state versions: a
    /// follower at each output's own version, an indexer synced from a snapshot at the snapshot's. A page from one
    /// resumes on the other without skipping an output.
    #[tokio::test]
    async fn a_fetch_resumes_on_an_indexer_that_stored_its_utxos_in_another_order() {
        use tari_template_lib_types::{UtxoAddress, UtxoId};

        use crate::storage_sqlite::models::UtxoUpdateRecord;

        // Output `seq` with the nonce and tag of `pair(1)`
        let sharing = |seq: u8, state_version: u64| {
            let UtxoUpdateRecord::Unspent(mut copy) = crate::storage_sqlite::unspent_at(1, state_version) else {
                unreachable!()
            };
            copy.address = UtxoAddress::new(crate::storage_sqlite::utxo_resource(), UtxoId::from_array([seq; 32]));
            UtxoUpdateRecord::Unspent(copy)
        };
        let (_follower_dir, follower) = crate::storage_sqlite::temp_store().await;
        follower
            .with_write_tx(move |tx| {
                tx.batch_insert_utxo_updates(Epoch(1), [1u8, 2, 3, 4].map(|seq| sharing(seq, 10 + u64::from(seq))))
            })
            .await
            .unwrap();
        let (_snapshot_dir, snapshot) = crate::storage_sqlite::temp_store().await;
        snapshot
            .with_write_tx(move |tx| tx.batch_insert_utxo_updates(Epoch(1), [4u8, 3, 2, 1].map(|seq| sharing(seq, 50))))
            .await
            .unwrap();
        let page = |store: SqliteIndexerStore, cursor: Option<UtxoId>| async move {
            store
                .with_read_tx(move |tx| {
                    read_utxos(tx, &crate::storage_sqlite::utxo_resource(), &[pair(1)], &[], cursor, 2)
                })
                .await
                .unwrap()
        };
        let ids = |fetched: &FetchedUtxos| fetched.utxos.iter().map(|(id, _)| *id).collect::<Vec<_>>();

        let first = page(follower.clone(), None).await;
        assert_eq!(ids(&first), [UtxoId::from_array([1; 32]), UtxoId::from_array([2; 32])]);
        let second = page(snapshot.clone(), first.next_cursor).await;
        assert_eq!(ids(&second), [UtxoId::from_array([3; 32]), UtxoId::from_array([4; 32])]);
        assert_eq!(second.next_cursor, None);
    }

    #[test]
    fn an_unproven_down_is_an_error() {
        let id = substate();
        let req = SubstateRequirementRef::new(&id, Some(SubstateVersion::new(3)));

        let err = SubstateManager::to_proven_lookup(req, down(None)).unwrap_err();
        assert!(
            matches!(err, SubstateManagerError::InputSubstateIsDown { version, .. } if version == SubstateVersion::new(3)),
            "{err}"
        );
    }
}
