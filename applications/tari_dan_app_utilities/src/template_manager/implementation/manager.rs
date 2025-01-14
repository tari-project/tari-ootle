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

use std::{collections::HashMap, convert::TryFrom, fs, sync::Arc};

use chrono::Utc;
use log::*;
use tari_common_types::types::{FixedHash, PublicKey};
use tari_crypto::tari_utilities::ByteArray;
use tari_dan_common_types::{
    optional::Optional,
    services::template_provider::TemplateProvider,
    Epoch,
    NodeAddressable,
};
use tari_dan_engine::{
    flow::FlowFactory,
    function_definitions::FlowFunctionDefinition,
    template::{LoadedTemplate, TemplateModuleLoader},
    wasm::WasmModule,
};
use tari_dan_storage::global::{DbTemplate, DbTemplateType, DbTemplateUpdate, GlobalDb, TemplateStatus};
use tari_dan_storage_sqlite::global::SqliteGlobalDbAdapter;
use tari_engine_types::{calculate_template_binary_hash, hashing::template_hasher32};
use tari_template_builtin::{
    get_template_builtin,
    ACCOUNT_NFT_TEMPLATE_ADDRESS,
    ACCOUNT_TEMPLATE_ADDRESS,
    FAUCET_TEMPLATE_ADDRESS,
};
use tari_template_lib::{models::TemplateAddress, Hash};

use super::TemplateConfig;
use crate::template_manager::{
    implementation::cmap_semaphore,
    interface::{Template, TemplateExecutable, TemplateManagerError, TemplateMetadata},
};

const LOG_TARGET: &str = "tari::validator_node::template_manager";

const CONCURRENT_ACCESS_LIMIT: isize = 100;

#[derive(Debug)]
pub struct TemplateManager<TAddr> {
    global_db: GlobalDb<SqliteGlobalDbAdapter<TAddr>>,
    config: TemplateConfig,
    builtin_templates: Arc<HashMap<TemplateAddress, Template>>,
    cache: mini_moka::sync::Cache<TemplateAddress, LoadedTemplate>,
    cmap_semaphore: cmap_semaphore::ConcurrentMapSemaphore<TemplateAddress>,
}

impl<TAddr: NodeAddressable> TemplateManager<TAddr> {
    pub fn initialize(
        global_db: GlobalDb<SqliteGlobalDbAdapter<TAddr>>,
        config: TemplateConfig,
    ) -> Result<Self, TemplateManagerError> {
        // load the builtin account templates
        let builtin_templates = Self::load_builtin_templates();
        let cache = mini_moka::sync::Cache::builder()
            .weigher(|_, t: &LoadedTemplate| u32::try_from(t.code_size()).unwrap_or(u32::MAX))
            .max_capacity(config.max_cache_size_bytes())
            .build();

        // Precache builtins
        for addr in builtin_templates.keys() {
            cache.insert(*addr, WasmModule::load_template_from_code(get_template_builtin(addr))?);
        }

        Ok(Self {
            global_db,
            builtin_templates: Arc::new(builtin_templates),
            cache,
            config,
            cmap_semaphore: cmap_semaphore::ConcurrentMapSemaphore::new(CONCURRENT_ACCESS_LIMIT),
        })
    }

    fn load_builtin_templates() -> HashMap<TemplateAddress, Template> {
        // for now, we only load the "account" template
        let mut builtin_templates = HashMap::with_capacity(3);

        // get the builtin WASM code of the account template
        let compiled_code = get_template_builtin(&ACCOUNT_TEMPLATE_ADDRESS);
        let template = Self::convert_code_to_template("Account", ACCOUNT_TEMPLATE_ADDRESS, compiled_code.to_vec());
        builtin_templates.insert(ACCOUNT_TEMPLATE_ADDRESS, template);

        // get the builtin WASM code of the account nft template
        let compiled_code = get_template_builtin(&ACCOUNT_NFT_TEMPLATE_ADDRESS);
        let template =
            Self::convert_code_to_template("AccountNft", ACCOUNT_NFT_TEMPLATE_ADDRESS, compiled_code.to_vec());
        builtin_templates.insert(ACCOUNT_NFT_TEMPLATE_ADDRESS, template);

        // get the builtin WASM code of the account nft template
        let compiled_code = get_template_builtin(&FAUCET_TEMPLATE_ADDRESS);
        let template = Self::convert_code_to_template("XtrFaucet", FAUCET_TEMPLATE_ADDRESS, compiled_code.to_vec());
        builtin_templates.insert(FAUCET_TEMPLATE_ADDRESS, template);

        builtin_templates
    }

    fn convert_code_to_template(name: &str, address: TemplateAddress, compiled_code: Vec<u8>) -> Template {
        // build the template object of the account template
        let binary_sha = calculate_template_binary_hash(&compiled_code);
        Template {
            metadata: TemplateMetadata {
                name: name.to_string(),
                address,
                binary_sha,
                author_public_key: Default::default(),
            },
            executable: TemplateExecutable::CompiledWasm(compiled_code),
        }
    }

    pub fn template_exists(
        &self,
        address: &TemplateAddress,
        status: Option<TemplateStatus>,
    ) -> Result<bool, TemplateManagerError> {
        if self.builtin_templates.contains_key(address) {
            return Ok(true);
        }
        let mut tx = self.global_db.create_transaction()?;
        self.global_db
            .templates(&mut tx)
            .template_exists(address, status)
            .map_err(|_| TemplateManagerError::TemplateNotFound { address: *address })
    }

    /// Deletes a template if exists.
    pub fn delete_template(&self, address: &TemplateAddress) -> Result<(), TemplateManagerError> {
        if !self.template_exists(address, None)? {
            return Ok(());
        }

        let mut tx = self.global_db.create_transaction()?;
        self.global_db
            .templates(&mut tx)
            .delete_template(address)
            .map_err(|_| TemplateManagerError::TemplateDeleteFailed { address: *address })
    }

    /// Fetching all templates by addresses.
    pub fn fetch_templates_by_addresses(
        &self,
        mut addresses: Vec<TemplateAddress>,
    ) -> Result<Vec<Template>, TemplateManagerError> {
        let mut result = Vec::with_capacity(addresses.len());

        // check in built-in templates first
        let mut found_template_indexes = vec![];
        for (i, address) in addresses.iter().enumerate() {
            if let Some(template) = self.builtin_templates.get(address) {
                result.push(template.clone());
                found_template_indexes.push(i);
            }
        }
        found_template_indexes.iter().for_each(|i| {
            addresses.remove(*i);
        });

        // check the rest in DB
        let mut tx = self.global_db.create_transaction()?;
        self.global_db
            .templates(&mut tx)
            .get_templates_by_addresses(addresses.iter().map(|addr| addr.as_ref()).collect())
            .map_err(|_| TemplateManagerError::TemplatesNotFound { addresses })?
            .iter()
            .for_each(|template| {
                result.push(Template::from(template.clone()));
            });

        Ok(result)
    }

    pub fn fetch_template(&self, address: &TemplateAddress) -> Result<Template, TemplateManagerError> {
        // first of all, check if the address is for a bulitin template
        if let Some(template) = self.builtin_templates.get(address) {
            return Ok(template.to_owned());
        }

        let mut tx = self.global_db.create_transaction()?;
        let template = self
            .global_db
            .templates(&mut tx)
            .get_template(address)?
            .ok_or(TemplateManagerError::TemplateNotFound { address: *address })?;

        if !matches!(template.status, TemplateStatus::Active | TemplateStatus::Deprecated) {
            return Err(TemplateManagerError::TemplateUnavailable);
        }

        // first check debug
        if let Some(dbg_replacement) = self.config.debug_replacements().get(address) {
            let mut result: Template = template.into();
            match &mut result.executable {
                TemplateExecutable::CompiledWasm(wasm) => {
                    let binary = fs::read(dbg_replacement).expect("Could not read debug file");
                    *wasm = binary;
                },
                TemplateExecutable::Flow(_) => {
                    todo!("debug replacements for flow templates not implemented");
                },
                _ => return Err(TemplateManagerError::TemplateUnavailable),
            }

            Ok(result)
        } else {
            Ok(template.into())
        }
    }

    pub fn fetch_template_metadata(&self, limit: usize) -> Result<Vec<TemplateMetadata>, TemplateManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        // TODO: we should be able to fetch just the metadata and not the compiled code
        let templates = self.global_db.templates(&mut tx).get_templates(limit)?;
        let mut templates: Vec<TemplateMetadata> = templates.into_iter().map(Into::into).collect();
        let mut builtin_metadata: Vec<TemplateMetadata> =
            self.builtin_templates.values().map(|t| t.metadata.to_owned()).collect();
        templates.append(&mut builtin_metadata);

        Ok(templates)
    }

    pub fn add_pending_template(
        &self,
        template_address: tari_engine_types::TemplateAddress,
        epoch: Epoch,
    ) -> Result<(), TemplateManagerError> {
        let template = DbTemplate::empty_pending(template_address, epoch);

        let mut tx = self.global_db.create_transaction()?;
        let mut templates_db = self.global_db.templates(&mut tx);
        match templates_db.get_template(&template.template_address)? {
            Some(_) => templates_db.update_template(
                &template.template_address,
                DbTemplateUpdate::status(TemplateStatus::Pending),
            )?,
            None => templates_db.insert_template(template)?,
        }

        tx.commit()?;

        Ok(())
    }

    pub(super) fn add_template(
        &self,
        author_public_key: PublicKey,
        template_address: tari_engine_types::TemplateAddress,
        template: TemplateExecutable,
        template_name: Option<String>,
        template_status: Option<TemplateStatus>,
        epoch: Epoch,
    ) -> Result<(), TemplateManagerError> {
        enum TemplateHash {
            Hash(Hash),
            FixedHash(FixedHash),
        }

        let mut compiled_code = None;
        let mut flow_json = None;
        let mut manifest = None;
        let mut template_type = DbTemplateType::Wasm;
        let template_hash: TemplateHash;
        let mut template_name = template_name.unwrap_or(String::from("default"));
        let mut template_url = None;
        match template {
            TemplateExecutable::CompiledWasm(binary) => {
                let loaded_template = WasmModule::load_template_from_code(binary.as_slice())?;
                template_hash = TemplateHash::Hash(template_hasher32().chain(binary.as_slice()).result());
                compiled_code = Some(binary);
                template_name = loaded_template.template_name().to_string();
            },
            TemplateExecutable::Manifest(curr_manifest) => {
                template_hash = TemplateHash::Hash(template_hasher32().chain(curr_manifest.as_str()).result());
                manifest = Some(curr_manifest);
                template_type = DbTemplateType::Manifest;
            },
            TemplateExecutable::Flow(curr_flow_json) => {
                template_hash = TemplateHash::Hash(template_hasher32().chain(curr_flow_json.as_str()).result());
                flow_json = Some(curr_flow_json);
                template_type = DbTemplateType::Flow;
            },
            TemplateExecutable::DownloadableWasm(url, hash) => {
                template_url = Some(url.to_string());
                template_type = DbTemplateType::Wasm;
                template_hash = TemplateHash::FixedHash(hash);
            },
        }

        let template = DbTemplate {
            author_public_key: FixedHash::try_from(author_public_key.to_vec().as_slice())?,
            template_name,
            template_address,
            expected_hash: match template_hash {
                TemplateHash::Hash(hash) => FixedHash::from(hash.into_array()),
                TemplateHash::FixedHash(hash) => hash,
            },
            status: template_status.unwrap_or(TemplateStatus::New),
            compiled_code,
            added_at: Utc::now().naive_utc(),
            template_type,
            flow_json,
            manifest,
            url: template_url,
            epoch,
        };

        let mut tx = self.global_db.create_transaction()?;
        let mut templates_db = self.global_db.templates(&mut tx);
        if templates_db.get_template(&template.template_address)?.is_some() {
            return Ok(());
        }
        templates_db.insert_template(template)?;
        tx.commit()?;

        Ok(())
    }

    pub(super) fn update_template(
        &self,
        address: TemplateAddress,
        update: DbTemplateUpdate,
    ) -> Result<(), TemplateManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        let mut template_db = self.global_db.templates(&mut tx);
        template_db.update_template(&address, update)?;
        tx.commit()?;

        Ok(())
    }

    pub(super) fn fetch_pending_templates(&self) -> Result<Vec<DbTemplate>, TemplateManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        let templates = self.global_db.templates(&mut tx).get_pending_templates(1000)?;
        Ok(templates)
    }
}

impl<TAddr: NodeAddressable + Send + Sync + 'static> TemplateProvider for TemplateManager<TAddr> {
    type Error = TemplateManagerError;
    type Template = LoadedTemplate;

    fn get_template_module(&self, address: &TemplateAddress) -> Result<Option<Self::Template>, Self::Error> {
        if let Some(template) = self.cache.get(address) {
            debug!(target: LOG_TARGET, "CACHE HIT: Template {}", address);
            return Ok(Some(template));
        }

        // This protects the following critical area by:
        // 1. preventing more than CONCURRENT_ACCESS_LIMIT concurrent accesses
        // 2. preventing more than one load of the same template
        // The reasons are:
        // 1. for efficiency, to only ever load the template once (until it is purged from the cache), and
        // 2. to prevent stack overflow. This happens in stress testing, if around 200 templates are loaded concurrently
        let guard = self.cmap_semaphore.acquire(*address);
        let _access = guard.access();

        if let Some(template) = self.cache.get(address) {
            debug!(target: LOG_TARGET, "CACHE HIT: Template {}", address);
            return Ok(Some(template));
        }

        let Some(template) = self.fetch_template(address).optional()? else {
            return Ok(None);
        };
        debug!(target: LOG_TARGET, "CACHE MISS: Template {}", address);
        let loaded = match template.executable {
            TemplateExecutable::CompiledWasm(wasm) => {
                let module = WasmModule::from_code(wasm);
                module.load_template()?
            },
            TemplateExecutable::Manifest(_) => return Err(TemplateManagerError::UnsupportedTemplateType),
            TemplateExecutable::Flow(flow_json) => {
                let definition: FlowFunctionDefinition = serde_json::from_str(&flow_json)?;
                let factory = FlowFactory::try_create::<Self>(definition)?;
                LoadedTemplate::Flow(factory)
            },
            TemplateExecutable::DownloadableWasm(_, _) => {
                // impossible case, since there is no separate downloadable wasm type in DB level
                return Err(Self::Error::UnsupportedTemplateType);
            },
        };

        self.cache.insert(*address, loaded.clone());

        Ok(Some(loaded))
    }
}

impl<TAddr> Clone for TemplateManager<TAddr> {
    fn clone(&self) -> Self {
        Self {
            global_db: self.global_db.clone(),
            config: self.config.clone(),
            builtin_templates: self.builtin_templates.clone(),
            cache: self.cache.clone(),
            cmap_semaphore: self.cmap_semaphore.clone(),
        }
    }
}
