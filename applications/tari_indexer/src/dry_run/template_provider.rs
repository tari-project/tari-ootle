//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{io, path::PathBuf};

use tari_engine::wasm::DiskCachedWasmTemplateProvider;
use tari_engine_types::{
    published_template::{PublishedTemplate, PublishedTemplateAddress},
    substate::{Substate, SubstateId},
};
use tari_ootle_common_types::{
    SubstateRequirementRef,
    optional::Optional,
    services::template_provider::TemplateProvider,
};
use tari_ootle_template_provider::{MemoryCacheTemplateProvider, TemplateConfig};
use tari_template_lib_types::TemplateAddress;
use tokio::runtime::Handle;

use crate::substate_manager::{SubstateManager, SubstateManagerError};

/// The dry-run template provider chain, mirroring the validator node:
/// `[in-memory cache + concurrent-load coalescing] -> [precompiled-module disk cache] -> [network fetch]`.
///
/// It deliberately does not read or write the global-db `templates` table, so that table and its associated
/// `TemplateManager` code can be retired independently.
pub type DryRunTemplateProvider = MemoryCacheTemplateProvider<DiskCachedWasmTemplateProvider<NetworkTemplateProvider>>;

/// Builds the shared dry-run template provider. `handle` must belong to the runtime that dry runs execute on;
/// `wasm_cache_dir` is the on-disk compiled-module cache directory.
pub fn build_dry_run_template_provider(
    handle: Handle,
    substate_manager: SubstateManager,
    wasm_cache_dir: impl Into<PathBuf>,
) -> io::Result<DryRunTemplateProvider> {
    let network = NetworkTemplateProvider::new(handle, substate_manager);
    let disk_cached = DiskCachedWasmTemplateProvider::open(network, wasm_cache_dir)?;
    Ok(MemoryCacheTemplateProvider::new(
        disk_cached,
        &TemplateConfig::default(),
    ))
}

/// Resolves a published template by fetching its substate from the network on demand.
///
/// This is the innermost layer of the dry-run provider chain. A dry run cannot know its full template set up
/// front: native instructions (e.g. account creation) reference builtin templates that never appear in the
/// transaction, and a template may make a cross-template call to an address hard-coded in its own code. So each
/// requested address is fetched from the network when the surrounding caches miss. Builtins are served by the
/// in-memory cache's precache (as on the validator node), so they do not reach this layer in practice.
///
/// The engine calls [`TemplateProvider::get_template`] synchronously from blocking WASM execution, whereas the
/// fetch is async, so it is bridged onto the async runtime via `handle`.
#[derive(Clone)]
pub struct NetworkTemplateProvider {
    handle: Handle,
    substate_manager: SubstateManager,
}

impl NetworkTemplateProvider {
    pub fn new(handle: Handle, substate_manager: SubstateManager) -> Self {
        Self {
            handle,
            substate_manager,
        }
    }

    /// Fetches a published-template substate, blocking the calling (synchronous) thread until the async fetch
    /// completes. The engine only invokes `get_template` from blocking execution, so this never blocks a runtime
    /// worker. Returns `None` if no substate exists at the address.
    fn fetch_blocking(&self, address: TemplateAddress) -> Result<Option<Substate>, NetworkTemplateProviderError> {
        let substate_manager = self.substate_manager.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.handle.spawn(async move {
            let id = SubstateId::from(PublishedTemplateAddress::from_template_address(address));
            let result = substate_manager
                .get_substate(SubstateRequirementRef::unversioned(&id))
                .await;
            // The receiver is only dropped if the dry run was abandoned; the send result is irrelevant then.
            drop(tx.send(result));
        });
        rx.recv()
            .map_err(|_| NetworkTemplateProviderError::FetchTaskDropped)?
            .optional()
            .map_err(NetworkTemplateProviderError::Substate)
    }
}

impl TemplateProvider for NetworkTemplateProvider {
    type Error = NetworkTemplateProviderError;
    type Template = PublishedTemplate;

    fn get_template(&self, address: &TemplateAddress) -> Result<Option<Self::Template>, Self::Error> {
        let Some(substate) = self.fetch_blocking(*address)? else {
            return Ok(None);
        };
        let published = substate
            .into_substate_value()
            .into_template()
            .ok_or(NetworkTemplateProviderError::NotATemplate { address: *address })?;
        Ok(Some(published))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NetworkTemplateProviderError {
    #[error("Failed to fetch template substate: {0}")]
    Substate(#[source] SubstateManagerError),
    #[error("Substate at address {address} is not a published template")]
    NotATemplate { address: TemplateAddress },
    #[error("Template fetch task terminated without returning a result")]
    FetchTaskDropped,
}
