//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::debug;
use serde::{Deserialize, Serialize};
use tari_engine::{
    template::{LoadedTemplate, TemplateLoaderError},
    wasm::WasmModule,
};
use tari_engine_types::published_template::PublishedTemplate;
use tari_ootle_common_types::{
    services::template_provider::{TemplateMetadataProvider, TemplateProvider, TemplateProviderMetadata},
    Epoch,
};
use tari_template_builtin::{
    get_template_builtin,
    ACCOUNT_TEMPLATE_ADDRESS,
    NFT_FAUCET_TEMPLATE_ADDRESS,
    XTR_FAUCET_TEMPLATE_ADDRESS,
};
use tari_template_lib::prelude::TemplateAddress;

use crate::cmap_semaphore;

const LOG_TARGET: &str = "tari::validator::state_store_template_provider";
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

#[derive(Clone)]
pub struct StateStoreTemplateProvider<TStore> {
    inner: TStore,
    cache: mini_moka::sync::Cache<TemplateAddress, LoadedTemplate>,
    cmap_semaphore: cmap_semaphore::ConcurrentMapSemaphore<TemplateAddress>,
}

impl<TStore: TemplateProvider<Template = PublishedTemplate>> StateStoreTemplateProvider<TStore> {
    pub fn new(inner: TStore, config: &TemplateConfig) -> Self {
        // load the builtin account templates
        let cache = mini_moka::sync::Cache::builder()
            .weigher(|_, t: &LoadedTemplate| u32::try_from(t.code_size()).unwrap_or(u32::MAX))
            .max_capacity(config.max_cache_size_bytes())
            .initial_capacity(3)
            .build();

        // Precache builtins
        for (addr, code) in Self::get_builtin_templates() {
            cache.insert(
                *addr,
                WasmModule::load_template_from_code(code).expect("Built-in template failed to load"),
            );
        }

        Self {
            inner,
            cache,
            cmap_semaphore: cmap_semaphore::ConcurrentMapSemaphore::new(CONCURRENT_ACCESS_LIMIT),
        }
    }

    fn get_builtin_templates() -> impl Iterator<Item = (&'static TemplateAddress, &'static [u8])> {
        [
            (
                &ACCOUNT_TEMPLATE_ADDRESS,
                get_template_builtin(&ACCOUNT_TEMPLATE_ADDRESS),
            ),
            (
                &NFT_FAUCET_TEMPLATE_ADDRESS,
                get_template_builtin(&NFT_FAUCET_TEMPLATE_ADDRESS),
            ),
            (
                &XTR_FAUCET_TEMPLATE_ADDRESS,
                get_template_builtin(&XTR_FAUCET_TEMPLATE_ADDRESS),
            ),
        ]
        .into_iter()
    }
}

impl<TStore: TemplateProvider<Template = PublishedTemplate>> TemplateProvider for StateStoreTemplateProvider<TStore> {
    type Error = StateStoreTemplateProviderError;
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

        let Some(template) = self
            .inner
            .get_template(address)
            .map_err(|e| StateStoreTemplateProviderError::InnerProvider(e.into()))?
        else {
            return Ok(None);
        };

        let loaded = WasmModule::load_template_from_code(template.binary.as_slice())?;

        self.cache.insert(*address, loaded.clone());

        Ok(Some(loaded))
    }

    fn has_template(&self, id: &TemplateAddress) -> Result<bool, Self::Error> {
        Ok(self.cache.contains_key(id) ||
            self.inner
                .has_template(id)
                .map_err(|e| StateStoreTemplateProviderError::InnerProvider(e.into()))?)
    }
}
impl<TStore: TemplateProvider<Template = PublishedTemplate>> TemplateMetadataProvider
    for StateStoreTemplateProvider<TStore>
{
    fn get_template_metadata(&self, id: &TemplateAddress) -> Result<Option<TemplateProviderMetadata>, Self::Error> {
        let template = self
            .inner
            .get_template(id)
            .map_err(|e| StateStoreTemplateProviderError::InnerProvider(e.into()))?;

        Ok(template.map(|t| TemplateProviderMetadata {
            author: t.author,
            binary_hash: t.to_binary_hash(),
            epoch: Epoch(t.at_epoch),
        }))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StateStoreTemplateProviderError {
    #[error(transparent)]
    InnerProvider(anyhow::Error),
    #[error("Template load error: {0}")]
    TemplateLoadError(#[from] TemplateLoaderError),
}
