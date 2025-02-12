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
    convert::TryFrom,
    fs,
    sync::Arc,
};

use chrono::Utc;
use log::*;
use tari_common_types::types::{FixedHash, PublicKey};
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
use tari_dan_p2p::proto::rpc::TemplateType;
use tari_dan_storage::global::{DbTemplate, DbTemplateType, DbTemplateUpdate, GlobalDb, TemplateStatus};
use tari_dan_storage_sqlite::global::SqliteGlobalDbAdapter;
use tari_engine_types::{calculate_template_binary_hash, hashing::hash_template_code};
use tari_template_builtin::{
    get_template_builtin,
    ACCOUNT_NFT_TEMPLATE_ADDRESS,
    ACCOUNT_TEMPLATE_ADDRESS,
    FAUCET_TEMPLATE_ADDRESS,
};
use tari_template_lib::models::TemplateAddress;

use super::{convert_to_db_template_type, TemplateConfig};
use crate::{
    implementation::cmap_semaphore,
    interface::{Template, TemplateExecutable, TemplateManagerError, TemplateMetadata, TemplateQueryResult},
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
            if status.is_some_and(|s| !s.is_active()) {
                return Ok(false);
            }

            return Ok(true);
        }
        let mut tx = self.global_db.create_transaction()?;
        let exists = self.global_db.templates(&mut tx).template_exists(address, status)?;
        Ok(exists)
    }

    /// Deletes a template if exists.
    pub fn deprecate_template(&self, address: &TemplateAddress) -> Result<(), TemplateManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        self.global_db
            .templates(&mut tx)
            .set_status(address, TemplateStatus::Deprecated)?;
        Ok(())
    }

    /// Fetching all templates by addresses.
    pub fn fetch_templates_by_addresses(
        &self,
        addresses: Vec<TemplateAddress>,
    ) -> Result<Vec<TemplateQueryResult>, TemplateManagerError> {
        let mut results = Vec::with_capacity(addresses.len());

        let mut unique_addrs = addresses.iter().collect::<HashSet<_>>();
        // check in built-in templates first
        for address in &addresses {
            if let Some(template) = self.builtin_templates.get(address) {
                unique_addrs.remove(address);
                results.push(TemplateQueryResult::Found {
                    template: template.clone(),
                });
            }
        }

        // check the rest in DB
        let mut tx = self.global_db.create_transaction()?;
        let templates = self
            .global_db
            .templates(&mut tx)
            .get_templates_by_addresses(unique_addrs.iter().map(|addr| addr.as_ref()).collect())?;

        for template in templates {
            unique_addrs.remove(&template.template_address);
            if template.status.is_active() || template.status.is_deprecated() {
                results.push(TemplateQueryResult::Found {
                    template: template.try_into()?,
                });
            } else {
                results.push(TemplateQueryResult::NotAvailable {
                    address: template.template_address,
                    status: template.status,
                });
            }
        }

        for address in unique_addrs {
            results.push(TemplateQueryResult::NotFound { address: *address })
        }

        Ok(results)
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
            return Err(TemplateManagerError::TemplateUnavailable {
                status: Some(template.status),
            });
        }

        // first check debug
        if let Some(dbg_replacement) = self.config.debug_replacements().get(address) {
            let mut result: Template = template.try_into()?;
            match &mut result.executable {
                TemplateExecutable::CompiledWasm(wasm) => {
                    let binary = fs::read(dbg_replacement).expect("Could not read debug file");
                    *wasm = binary;
                },
                TemplateExecutable::Flow(_) => {
                    todo!("debug replacements for flow templates not implemented");
                },
                _ => return Err(TemplateManagerError::TemplateUnavailable { status: None }),
            }

            Ok(result)
        } else {
            Ok(template.try_into()?)
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
        template_name: String,
        template_address: TemplateAddress,
        author_public_key: PublicKey,
        binary_hash: FixedHash,
        epoch: Epoch,
        template_type: TemplateType,
    ) -> Result<(), TemplateManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        let mut templates_db = self.global_db.templates(&mut tx);
        if let Some(template) = templates_db.get_template(&template_address)? {
            if template.status.is_active() || template.status.is_deprecated() {
                return Err(TemplateManagerError::TemplateUnavailable {
                    status: Some(template.status),
                });
            }
            templates_db.update_template(&template.template_address, DbTemplateUpdate {
                author_public_key: Some(author_public_key),
                expected_hash: Some(binary_hash),
                template_type: Some(convert_to_db_template_type(template_type)),
                template_name: Some(template_name),
                status: Some(TemplateStatus::Pending),
                epoch: Some(epoch),
                ..Default::default()
            })?;
        } else {
            let template = DbTemplate {
                author_public_key,
                template_address,
                template_name,
                expected_hash: binary_hash,
                template_type: convert_to_db_template_type(template_type),
                code: None,
                url: None,
                status: TemplateStatus::Pending,
                epoch,
                added_at: Default::default(),
            };
            templates_db.insert_template(template)?
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
        let mut code = None;
        let mut template_type = DbTemplateType::Wasm;
        let template_hash;
        let mut template_name = template_name.unwrap_or(String::from("default"));
        let mut template_url = None;
        match template {
            TemplateExecutable::CompiledWasm(binary) => {
                let loaded_template = WasmModule::load_template_from_code(binary.as_slice())?;
                template_hash = hash_template_code(binary.as_slice()).into_array().into();
                code = Some(binary);
                template_name = loaded_template.template_name().to_string();
            },
            TemplateExecutable::Manifest(curr_manifest) => {
                template_hash = hash_template_code(curr_manifest.as_bytes()).into_array().into();
                code = Some(curr_manifest.into_bytes());
                template_type = DbTemplateType::Manifest;
            },
            TemplateExecutable::Flow(curr_flow_json) => {
                template_hash = hash_template_code(curr_flow_json.as_bytes()).into_array().into();
                code = Some(curr_flow_json.into_bytes());
                template_type = DbTemplateType::Flow;
            },
            TemplateExecutable::DownloadableWasm(url, hash) => {
                template_url = Some(url.to_string());
                template_type = DbTemplateType::Wasm;
                template_hash = hash;
            },
        }

        let template = DbTemplate {
            author_public_key,
            template_name,
            template_address,
            expected_hash: template_hash,
            status: template_status.unwrap_or(TemplateStatus::New),
            code,
            added_at: Utc::now().naive_utc(),
            template_type,
            url: template_url,
            epoch,
        };

        let mut tx = self.global_db.create_transaction()?;
        let mut templates_db = self.global_db.templates(&mut tx);
        if templates_db.template_exists(&template.template_address, None)? {
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
