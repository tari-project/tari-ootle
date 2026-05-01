//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::debug;
use serde::{Deserialize, Serialize};
use tari_engine::{template::LoadedTemplate, wasm::WasmModule};
use tari_ootle_common_types::services::template_provider::{
    TemplateMetadataProvider,
    TemplateProvider,
    TemplateProviderMetadata,
};
use tari_template_builtin::all_builtin_templates;
use tari_template_lib::types::TemplateAddress;

use crate::cmap_semaphore;

const LOG_TARGET: &str = "tari::validator::memory_cache_template_provider";
const CONCURRENT_ACCESS_LIMIT: isize = 100;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TemplateConfig {
    max_cache_size_bytes: u64,
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            max_cache_size_bytes: 200 * 1024 * 1024,
        }
    }
}

impl TemplateConfig {
    pub fn max_cache_size_bytes(&self) -> u64 {
        self.max_cache_size_bytes
    }
}

/// Outermost layer of the validator-node template provider chain.
///
/// Holds an in-memory moka cache of `LoadedTemplate`s keyed by address and a
/// per-address semaphore to coalesce concurrent first-fetches. Builtin
/// templates are precompiled into the cache at construction time. Everything
/// else delegates to `inner` on miss — typically a
/// `tari_engine::wasm::DiskCachedWasmTemplateProvider` wrapping the raw state
/// store, so the layering is:
///
/// ```text
/// MemoryCacheTemplateProvider          (this)
///   └── DiskCachedWasmTemplateProvider (compiled-module disk cache + compile)
///         └── ValidatorNodeStateStore  (raw PublishedTemplate bytes from rocksdb)
/// ```
#[derive(Clone)]
pub struct MemoryCacheTemplateProvider<TInner> {
    inner: TInner,
    cache: mini_moka::sync::Cache<TemplateAddress, LoadedTemplate>,
    cmap_semaphore: cmap_semaphore::ConcurrentMapSemaphore<TemplateAddress>,
}

impl<TInner> MemoryCacheTemplateProvider<TInner>
where TInner: TemplateProvider<Template = LoadedTemplate>
{
    pub fn new(inner: TInner, config: &TemplateConfig) -> Self {
        let cache = mini_moka::sync::Cache::builder()
            .weigher(|_, t: &LoadedTemplate| u32::try_from(t.code_size()).unwrap_or(u32::MAX))
            .max_capacity(config.max_cache_size_bytes())
            .initial_capacity(all_builtin_templates().len())
            .build();

        // Precache builtins. Compile directly here — builtins live only in
        // memory, never go through the disk-cache layer (their addresses are
        // hardcoded constants and would otherwise pin stale compiled modules
        // across builtin recompiles). The disk-cache layer also has a
        // matching bypass on the lookup side as defence in depth.
        for template in all_builtin_templates() {
            cache.insert(
                template.address,
                WasmModule::load_template_from_code(template.binary).expect("Built-in template failed to load"),
            );
        }

        Self {
            inner,
            cache,
            cmap_semaphore: cmap_semaphore::ConcurrentMapSemaphore::new(CONCURRENT_ACCESS_LIMIT),
        }
    }
}

impl<TInner> TemplateProvider for MemoryCacheTemplateProvider<TInner>
where TInner: TemplateProvider<Template = LoadedTemplate> + Clone + 'static
{
    type Error = MemoryCacheTemplateProviderError;
    type Template = LoadedTemplate;

    fn get_template(&self, address: &TemplateAddress) -> Result<Option<Self::Template>, Self::Error> {
        if let Some(template) = self.cache.get(address) {
            debug!(target: LOG_TARGET, "CACHE HIT: Template {}", address);
            return Ok(Some(template));
        }
        debug!(target: LOG_TARGET, "CACHE MISS: Template {}", address);

        // This protects the following critical area by:
        // 1. preventing more than CONCURRENT_ACCESS_LIMIT concurrent accesses
        // 2. preventing more than one load of the same template
        // The reasons are:
        // 1. for efficiency, to only ever load the template once (until it is purged from the cache), and
        // 2. to prevent stack overflow. This happens in stress testing, if around 200 templates are loaded concurrently
        let guard = self.cmap_semaphore.acquire(*address);
        let _access = guard.access();

        // After acquiring the semaphore, the racing thread may have populated
        // the cache; check again before delegating to the inner provider.
        if let Some(template) = self.cache.get(address) {
            return Ok(Some(template));
        }

        let Some(loaded) = self
            .inner
            .get_template(address)
            .map_err(|e| MemoryCacheTemplateProviderError::Inner(e.into()))?
        else {
            return Ok(None);
        };

        self.cache.insert(*address, loaded.clone());
        Ok(Some(loaded))
    }

    fn has_template(&self, id: &TemplateAddress) -> Result<bool, Self::Error> {
        Ok(self.cache.contains_key(id) ||
            self.inner
                .has_template(id)
                .map_err(|e| MemoryCacheTemplateProviderError::Inner(e.into()))?)
    }
}

impl<TInner> TemplateMetadataProvider for MemoryCacheTemplateProvider<TInner>
where TInner: TemplateProvider<Template = LoadedTemplate> + TemplateMetadataProvider + Clone + 'static
{
    fn get_template_metadata(&self, id: &TemplateAddress) -> Result<Option<TemplateProviderMetadata>, Self::Error> {
        // The hot cache only stores compiled modules, not metadata fields.
        // Always delegate.
        self.inner
            .get_template_metadata(id)
            .map_err(|e| MemoryCacheTemplateProviderError::Inner(e.into()))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryCacheTemplateProviderError {
    #[error(transparent)]
    Inner(anyhow::Error),
}
