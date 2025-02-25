//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, time::Instant};

use indexmap::IndexMap;
use log::*;
use tari_dan_common_types::SubstateRequirement;
use tari_dan_engine::state_store::StateStoreError;
use tari_dan_storage::{consensus_models::SubstateRecord, StateStore, StorageError};
use tari_engine_types::substate::{Substate, SubstateId};
use tari_epoch_manager::{EpochManagerError, EpochManagerReader};
use tari_indexer_lib::{error::IndexerError, substate_cache::SubstateCache, substate_scanner::SubstateScanner};
use tari_transaction::Transaction;
use tari_validator_node_rpc::client::{SubstateResult, ValidatorNodeClientFactory};

use crate::p2p::services::mempool::{ResolvedSubstates, SubstateResolver};

const LOG_TARGET: &str = "tari::dan::substate_resolver";

#[derive(Debug, Clone)]
pub struct TariSubstateResolver<TStateStore, TEpochManager, TValidatorNodeClientFactory, TSubstateCache> {
    store: TStateStore,
    scanner: SubstateScanner<TEpochManager, TValidatorNodeClientFactory, TSubstateCache>,
}

impl<TStateStore, TEpochManager, TValidatorNodeClientFactory, TSubstateCache>
    TariSubstateResolver<TStateStore, TEpochManager, TValidatorNodeClientFactory, TSubstateCache>
where
    TStateStore: StateStore,
    TEpochManager: EpochManagerReader<Addr = TStateStore::Addr>,
    TValidatorNodeClientFactory: ValidatorNodeClientFactory<TStateStore::Addr>,
    TSubstateCache: SubstateCache,
{
    pub fn new(
        store: TStateStore,
        scanner: SubstateScanner<TEpochManager, TValidatorNodeClientFactory, TSubstateCache>,
    ) -> Self {
        Self { store, scanner }
    }

    fn resolve_local_substates(&self, transaction: &Transaction) -> Result<ResolvedSubstates, SubstateResolverError> {
        let inputs = transaction.all_inputs_substate_ids_iter();
        let (found_local_substates, missing_substate_ids) = self
            .store
            .with_read_tx(|tx| SubstateRecord::get_any_max_version(tx, inputs))?;

        // Reconcile requested inputs with found local substates
        let mut missing_substates = HashSet::with_capacity(missing_substate_ids.len());
        for requested_input in transaction.all_inputs_iter() {
            if missing_substate_ids.contains(requested_input.substate_id()) {
                // TODO/NOTE: This assumes that consensus is up to date (i.e. doesnt need to sync, or catch up). We need
                // to check the if the substate is in our shard range. The best action then may be to
                // let consensus handle it which is what happens currently anyway.
                missing_substates.insert(requested_input);
                // Not a local substate, so we will need to fetch it remotely
                continue;
            }

            match requested_input.version() {
                // Specific version requested
                Some(requested_version) => {
                    let maybe_match = found_local_substates
                        .iter()
                        .find(|s| s.substate_id() == requested_input.substate_id());

                    match maybe_match {
                        Some(substate) => {
                            if substate.version() < requested_version {
                                return Err(SubstateResolverError::InputSubstateDoesNotExist {
                                    substate_requirement: requested_input.to_owned(),
                                });
                            }

                            if substate.is_destroyed() || substate.version() > requested_version {
                                return Err(SubstateResolverError::InputSubstateDowned {
                                    id: requested_input.substate_id().clone(),
                                    version: requested_version,
                                });
                            }

                            // OK
                        },
                        // Requested substate or version not found. We know that the requested substate is not foreign
                        // because we checked missing_substate_ids
                        None => {
                            return Err(SubstateResolverError::InputSubstateDoesNotExist {
                                substate_requirement: requested_input.to_owned(),
                            });
                        },
                    }
                },
                // No version specified, so we will use the latest version
                None => {
                    let substate = found_local_substates
                        .iter()
                        .find(| s| s.substate_id() == requested_input.substate_id())
                        // This is not possible
                        .ok_or_else(|| {
                            error!(
                                target: LOG_TARGET,
                                "🐞 BUG: Requested substate {} was not missing but was also not found",
                                requested_input.substate_id()
                            );
                            SubstateResolverError::InputSubstateDoesNotExist { substate_requirement: requested_input.to_owned() }
                        })?;

                    // Latest version is DOWN
                    if substate.is_destroyed() {
                        return Err(SubstateResolverError::InputSubstateDowned {
                            id: requested_input.substate_id().clone(),
                            version: substate.version(),
                        });
                    }

                    // User did not specify the version, so we will use the latest version
                    // Ok
                },
            }
        }

        info!(
            target: LOG_TARGET,
            "Found {} local substates and {} missing substates",
            found_local_substates.len(),
            missing_substate_ids.len(),
        );

        let mut substates = IndexMap::new();
        substates.extend(found_local_substates.into_iter().map(|s| {
            (
                s.substate_id.clone(),
                s.into_substate().expect("All substates already checked UP"),
            )
        }));

        Ok(ResolvedSubstates {
            local: substates,
            unresolved_foreign: missing_substates.into_iter().map(|s| s.to_owned()).collect(),
        })
    }

    async fn resolve_remote_substates(
        &self,
        requested_substates: &HashSet<SubstateRequirement>,
    ) -> Result<IndexMap<SubstateId, Substate>, SubstateResolverError> {
        let mut substates = IndexMap::with_capacity(requested_substates.len());
        for substate_req in requested_substates {
            let timer = Instant::now();
            let substate_result = self
                .scanner
                .get_substate(substate_req.substate_id(), substate_req.version())
                .await?;

            match substate_result {
                SubstateResult::Up { id, substate, .. } => {
                    info!(
                        target: LOG_TARGET,
                        "Retrieved substate {} in {}ms",
                        id,
                        timer.elapsed().as_millis()
                    );
                    substates.insert(id, substate);
                },
                SubstateResult::Down { id, version, .. } => {
                    return Err(SubstateResolverError::InputSubstateDowned { id, version });
                },
                SubstateResult::DoesNotExist => {
                    return Err(SubstateResolverError::InputSubstateDoesNotExist {
                        substate_requirement: substate_req.clone(),
                    });
                },
            }
        }

        Ok(substates)
    }
}

impl<TStateStore, TEpochManager, TValidatorNodeClientFactory, TSubstateCache> SubstateResolver
    for TariSubstateResolver<TStateStore, TEpochManager, TValidatorNodeClientFactory, TSubstateCache>
where
    TStateStore: StateStore + Sync + Send,
    TEpochManager: EpochManagerReader<Addr = TStateStore::Addr>,
    TValidatorNodeClientFactory: ValidatorNodeClientFactory<TStateStore::Addr>,
    TSubstateCache: SubstateCache,
{
    type Error = SubstateResolverError;

    fn try_resolve_local(&self, transaction: &Transaction) -> Result<ResolvedSubstates, Self::Error> {
        self.resolve_local_substates(transaction)
    }

    async fn try_resolve_foreign(
        &self,
        requested_substates: &HashSet<SubstateRequirement>,
    ) -> Result<IndexMap<SubstateId, Substate>, Self::Error> {
        self.resolve_remote_substates(requested_substates).await
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SubstateResolverError {
    #[error("Storage error: {0}")]
    StorageError(#[from] StorageError),
    #[error("Indexer error: {0}")]
    IndexerError(#[from] IndexerError),
    #[error("Input substate does not exist: {substate_requirement}")]
    InputSubstateDoesNotExist { substate_requirement: SubstateRequirement },
    #[error("Input substate is downed: {id} (version: {version})")]
    InputSubstateDowned { id: SubstateId, version: u32 },
    #[error("Epoch manager error: {0}")]
    EpochManagerError(#[from] EpochManagerError),
    #[error("State store error: {0}")]
    StateStorageError(#[from] StateStoreError),
}
