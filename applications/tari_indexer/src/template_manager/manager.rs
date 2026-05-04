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

use std::collections::{HashMap, HashSet};

use tari_engine::{
    template::LoadedTemplate,
    wasm::{WasmModule, WasmModuleCache},
};
use tari_engine_types::{
    calculate_template_binary_hash,
    hashing::hash_template_code,
    published_template::PublishedTemplateAddress,
    substate::SubstateId,
};
use tari_ootle_common_types::{
    Epoch,
    SubstateRequirementRef,
    optional::Optional,
    services::template_provider::TemplateProvider,
};
use tari_ootle_p2p::PeerAddress;
use tari_ootle_storage::{
    global::{DbTemplate, DbTemplateType, GlobalDb, TemplateStatus},
    time::{OffsetDateTime, PrimitiveDateTime},
};
use tari_ootle_storage_sqlite::global::SqliteGlobalDbAdapter;
use tari_template_builtin::{all_builtin_templates, is_builtin_template_address, try_get_template_builtin};
use tari_template_lib_types::{TemplateAddress, crypto::RistrettoPublicKeyBytes};

use super::{Template, TemplateCode, TemplateMetadata};
use crate::{substate_manager::SubstateManager, template_manager::error::TemplateManagerError};

#[derive(Debug, Clone)]
pub struct TemplateManager {
    global_db: GlobalDb<SqliteGlobalDbAdapter<PeerAddress>>,
    substate_manager: SubstateManager,
    wasm_cache: WasmModuleCache,
}

impl TemplateManager {
    pub fn initialize(
        global_db: GlobalDb<SqliteGlobalDbAdapter<PeerAddress>>,
        substate_manager: SubstateManager,
        wasm_cache: WasmModuleCache,
    ) -> Result<Self, TemplateManagerError> {
        // load the builtin templates
        let builtin_templates = Self::builtin_templates();

        // Load them into the database if they do not already exist
        let mut tx = global_db.create_transaction()?;
        for template in builtin_templates {
            let mut templates_db = global_db.templates(&mut tx);
            if !templates_db.template_exists(template.address(), None)? {
                let db_template = DbTemplate {
                    author_public_key: template.metadata.author_public_key,
                    template_address: *template.address(),
                    template_name: template.metadata.name.clone(),
                    binary_hash: template.metadata.binary_sha,
                    status: TemplateStatus::Active,
                    code: Some(template.code.as_raw_bytes().to_vec()),
                    url: None,
                    template_type: DbTemplateType::Wasm,
                    added_at: now(),
                    epoch: template.metadata.epoch,
                    metadata_hash: None,
                };
                templates_db.insert_template(db_template)?;
            }
        }

        tx.commit()?;

        Ok(Self {
            global_db,
            substate_manager,
            wasm_cache,
        })
    }

    /// Compile or deserialize the template binary for `address`. Result is
    /// persisted in the on-disk wasm cache so subsequent calls (across process
    /// restarts) skip the cranelift compile.
    ///
    /// Builtin templates bypass the cache: their addresses are hardcoded
    /// constants (independent of binary content), so a stale cache entry
    /// would silently serve an out-of-date compiled module after a builtin
    /// recompile. User-template addresses are content-addressed, so binary
    /// changes naturally invalidate the cache key.
    fn load_template_with_cache(
        &self,
        address: &TemplateAddress,
        code: &TemplateCode,
    ) -> Result<LoadedTemplate, TemplateManagerError> {
        if is_builtin_template_address(address) {
            return Ok(WasmModule::load_template_from_code(code.as_raw_bytes())?);
        }
        if let Some(loaded) = self.wasm_cache.try_load(address) {
            return Ok(loaded);
        }
        let loaded = WasmModule::load_template_from_code(code.as_raw_bytes())?;
        self.wasm_cache.store(address, &loaded);
        Ok(loaded)
    }

    fn builtin_templates() -> impl Iterator<Item = Template> {
        all_builtin_templates().iter().map(convert_builtin_template_from_code)
    }

    pub fn template_exists(
        &self,
        address: &TemplateAddress,
        status: Option<TemplateStatus>,
    ) -> Result<bool, TemplateManagerError> {
        if try_get_template_builtin(address).is_some() {
            if status.is_some_and(|s| !s.is_active()) {
                return Ok(false);
            }

            return Ok(true);
        }
        let mut tx = self.global_db.create_transaction()?;
        let exists = self.global_db.templates(&mut tx).template_exists(address, status)?;
        Ok(exists)
    }

    pub fn fetch_cached_template(&self, address: &TemplateAddress) -> Result<Template, TemplateManagerError> {
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

        Ok(template.try_into()?)
    }

    pub async fn fetch_and_load_template(
        &self,
        address: &TemplateAddress,
    ) -> Result<LoadedTemplate, TemplateManagerError> {
        let mut templates = self.fetch_and_load_templates([address]).await?;
        let template = templates
            .remove(address)
            .ok_or(TemplateManagerError::TemplateNotFound { address: *address })?;
        Ok(template)
    }

    pub fn fetch_template_metadata(&self, limit: usize) -> Result<Vec<TemplateMetadata>, TemplateManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        // TODO: we should be able to fetch just the metadata and not the compiled code
        let templates = self.global_db.templates(&mut tx).get_templates(limit)?;
        let templates = templates.into_iter().map(Into::into).collect();

        Ok(templates)
    }

    fn add_template(
        &self,
        template_name: String,
        author_public_key: RistrettoPublicKeyBytes,
        template_address: TemplateAddress,
        code: TemplateCode,
        template_status: TemplateStatus,
        epoch: Epoch,
        metadata_hash: Option<Vec<u8>>,
    ) -> Result<(), TemplateManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        let mut templates_db = self.global_db.templates(&mut tx);
        let code_bytes = code.into_raw_bytes();
        let template = DbTemplate {
            author_public_key,
            template_name,
            template_address,
            binary_hash: hash_template_code(&code_bytes).into_array().into(),
            status: template_status,
            code: Some(code_bytes.into_owned()),
            added_at: now(),
            template_type: DbTemplateType::Wasm,
            url: None,
            epoch,
            metadata_hash,
        };

        templates_db.insert_template(template)?;
        tx.commit()?;
        Ok(())
    }

    pub fn add_and_load_template(
        &self,
        author_public_key: RistrettoPublicKeyBytes,
        template_address: TemplateAddress,
        code: TemplateCode,
        template_status: TemplateStatus,
        epoch: Epoch,
        metadata_hash: Option<Vec<u8>>,
    ) -> Result<LoadedTemplate, TemplateManagerError> {
        let loaded_template = self.load_template_with_cache(&template_address, &code)?;

        self.add_template(
            loaded_template.template_name().to_string(),
            author_public_key,
            template_address,
            code,
            template_status,
            epoch,
            metadata_hash,
        )?;

        Ok(loaded_template)
    }

    pub async fn fetch_and_load_templates<'a, I: IntoIterator<Item = &'a TemplateAddress>>(
        &self,
        addresses: I,
    ) -> Result<HashMap<TemplateAddress, LoadedTemplate>, TemplateManagerError> {
        let template_addrs = addresses.into_iter().collect::<HashSet<_>>();

        let mut loaded_templates = HashMap::with_capacity(template_addrs.len());

        for template_addr in &template_addrs {
            if let Some(template) = self.fetch_cached_template(template_addr).optional()? {
                loaded_templates.insert(
                    **template_addr,
                    self.load_template_with_cache(template_addr, &template.code)?,
                );
            }
        }

        let substate_ids = template_addrs
            .into_iter()
            .filter(|addr| !loaded_templates.contains_key(addr))
            .map(|addr| SubstateId::from(PublishedTemplateAddress::from_template_address(*addr)))
            .collect::<Vec<_>>();
        let fetched_templates = self
            .substate_manager
            .get_substates(substate_ids.iter().map(SubstateRequirementRef::unversioned))
            .await?;
        if fetched_templates.len() != substate_ids.len() {
            let missing_ids = substate_ids
                .iter()
                .find(|id| !fetched_templates.contains_key(id))
                .cloned()
                .expect("There is at least one missing id");
            return Err(TemplateManagerError::TemplateNotFound {
                address: missing_ids
                    .as_template()
                    .expect("substate_ids are all PublishedTemplateAddress")
                    .as_template_address(),
            });
        }

        for (substate_id, substate) in fetched_templates {
            let template =
                substate
                    .into_substate_value()
                    .into_template()
                    .ok_or(TemplateManagerError::InvariantViolation {
                        details: format!("Expected template substate at address {}", substate_id),
                    })?;
            let template_addr = substate_id
                .as_template()
                .expect("fetched_templates are all templates")
                .as_template_address();

            let metadata_hash_bytes = template.metadata_hash.map(|h| h.to_bytes());
            let loaded = self.add_and_load_template(
                template.author,
                template_addr,
                TemplateCode::CompiledWasm(template.binary.into_bytes()),
                TemplateStatus::Active,
                Epoch(template.at_epoch),
                metadata_hash_bytes,
            )?;
            loaded_templates.insert(template_addr, loaded);
        }

        Ok(loaded_templates)
    }
}

impl TemplateProvider for TemplateManager {
    type Error = TemplateManagerError;
    type Template = LoadedTemplate;

    fn get_template(&self, address: &TemplateAddress) -> Result<Option<Self::Template>, Self::Error> {
        let Some(template) = self.fetch_cached_template(address).optional()? else {
            return Ok(None);
        };
        // Validate the template carries WASM code (the cache itself is keyed by
        // template address; the loader inside `load_template_with_cache` reads
        // `template.code.as_raw_bytes()` which is fine for WASM templates).
        if template.code.as_wasm_code().is_none() {
            return Err(TemplateManagerError::UnsupportedTemplateType);
        }
        let loaded = self.load_template_with_cache(address, &template.code)?;
        Ok(Some(loaded))
    }

    fn has_template(&self, address: &TemplateAddress) -> Result<bool, Self::Error> {
        Ok(self.template_exists(address, Some(TemplateStatus::Active))? ||
            self.template_exists(address, Some(TemplateStatus::Deprecated))?)
    }
}

fn now() -> PrimitiveDateTime {
    let now = OffsetDateTime::now_utc();
    PrimitiveDateTime::new(now.date(), now.time())
}

fn convert_builtin_template_from_code(template: &tari_template_builtin::Template) -> Template {
    let binary_sha = calculate_template_binary_hash(template.binary);
    Template {
        metadata: TemplateMetadata {
            name: template.name.to_string(),
            address: template.address,
            binary_sha: binary_sha.into_array().into(),
            author_public_key: Default::default(),
            code_size: template.binary.len(),
            epoch: Epoch::zero(),
            metadata_hash: None,
        },
        code: TemplateCode::StaticWasm(template.binary),
    }
}
