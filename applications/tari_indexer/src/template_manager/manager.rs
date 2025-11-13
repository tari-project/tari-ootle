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

use tari_engine::{
    template::{LoadedTemplate, TemplateModuleLoader},
    wasm::WasmModule,
};
use tari_engine_types::{calculate_template_binary_hash, hashing::hash_template_code};
use tari_ootle_common_types::{
    optional::Optional,
    services::template_provider::TemplateProvider,
    Epoch,
    NodeAddressable,
};
use tari_ootle_storage::{
    global::{DbTemplate, DbTemplateType, GlobalDb, TemplateStatus},
    time::{OffsetDateTime, PrimitiveDateTime},
};
use tari_ootle_storage_sqlite::global::SqliteGlobalDbAdapter;
use tari_template_builtin::{
    get_template_builtin,
    try_get_template_builtin,
    ACCOUNT_TEMPLATE_ADDRESS,
    NFT_FAUCET_TEMPLATE_ADDRESS,
    XTR_FAUCET_TEMPLATE_ADDRESS,
};
use tari_template_lib::types::{crypto::RistrettoPublicKeyBytes, TemplateAddress};

use super::{LoadedTemplateWithMetadata, Template, TemplateCode, TemplateConfig, TemplateMetadata};
use crate::template_manager::error::TemplateManagerError;

#[derive(Debug)]
pub struct TemplateManager<TAddr> {
    global_db: GlobalDb<SqliteGlobalDbAdapter<TAddr>>,
    config: TemplateConfig,
}

impl<TAddr: NodeAddressable> TemplateManager<TAddr> {
    pub fn initialize(
        global_db: GlobalDb<SqliteGlobalDbAdapter<TAddr>>,
        config: TemplateConfig,
    ) -> Result<Self, TemplateManagerError> {
        // load the builtin templates
        let builtin_templates = Self::builtin_templates();

        // Load them into the database if they do not already exist
        let mut tx = global_db.create_transaction()?;
        for (address, template) in builtin_templates {
            let mut templates_db = global_db.templates(&mut tx);
            if !templates_db.template_exists(&address, None)? {
                let db_template = DbTemplate {
                    author_public_key: template.metadata.author_public_key,
                    template_address: address,
                    template_name: template.metadata.name.clone(),
                    binary_hash: template.metadata.binary_sha,
                    status: TemplateStatus::Active,
                    code: Some(template.code.as_raw_bytes().to_vec()),
                    url: None,
                    template_type: DbTemplateType::Wasm,
                    added_at: now(),
                    epoch: template.metadata.epoch,
                };
                templates_db.insert_template(db_template)?;
            }
        }

        tx.commit()?;

        Ok(Self { global_db, config })
    }

    fn builtin_templates() -> impl Iterator<Item = (TemplateAddress, Template)> {
        [
            (
                ACCOUNT_TEMPLATE_ADDRESS,
                convert_builtin_template("Account", ACCOUNT_TEMPLATE_ADDRESS),
            ),
            (
                XTR_FAUCET_TEMPLATE_ADDRESS,
                convert_builtin_template("XtrFaucet", XTR_FAUCET_TEMPLATE_ADDRESS),
            ),
            (
                NFT_FAUCET_TEMPLATE_ADDRESS,
                convert_builtin_template("NftFaucet", NFT_FAUCET_TEMPLATE_ADDRESS),
            ),
        ]
        .into_iter()
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

    pub fn fetch_template(&self, address: &TemplateAddress) -> Result<Template, TemplateManagerError> {
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

    pub fn fetch_and_load_template(
        &self,
        address: &TemplateAddress,
    ) -> Result<LoadedTemplateWithMetadata, TemplateManagerError> {
        let template = self.fetch_template(address)?;
        let wasm = template
            .code
            .as_wasm_code()
            .ok_or(TemplateManagerError::UnsupportedTemplateType)?;
        let module = WasmModule::from_code(wasm);
        let loaded = module.load_template()?;
        Ok(LoadedTemplateWithMetadata {
            metadata: template.metadata,
            loaded,
        })
    }

    pub fn fetch_template_metadata(&self, limit: usize) -> Result<Vec<TemplateMetadata>, TemplateManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        // TODO: we should be able to fetch just the metadata and not the compiled code
        let templates = self.global_db.templates(&mut tx).get_templates(limit)?;
        let templates = templates.into_iter().map(Into::into).collect();

        Ok(templates)
    }

    pub fn add_and_load_template(
        &self,
        author_public_key: RistrettoPublicKeyBytes,
        template_address: TemplateAddress,
        code: TemplateCode,
        template_status: TemplateStatus,
        epoch: Epoch,
    ) -> Result<LoadedTemplate, TemplateManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        let mut templates_db = self.global_db.templates(&mut tx);
        let binary = code.as_raw_bytes();
        let loaded_template = WasmModule::load_template_from_code(binary)?;

        let template = DbTemplate {
            author_public_key,
            template_name: loaded_template.template_name().to_string(),
            template_address,
            binary_hash: hash_template_code(binary).into_array().into(),
            status: template_status,
            code: Some(binary.to_vec()),
            added_at: now(),
            template_type: DbTemplateType::Wasm,
            url: None,
            epoch,
        };

        templates_db.insert_template(template)?;
        tx.commit()?;
        Ok(loaded_template)
    }
}

impl<TAddr: NodeAddressable + Send + Sync + 'static> TemplateProvider for TemplateManager<TAddr> {
    type Error = TemplateManagerError;
    type Template = LoadedTemplate;

    fn get_template(&self, address: &TemplateAddress) -> Result<Option<Self::Template>, Self::Error> {
        let Some(template) = self.fetch_template(address).optional()? else {
            return Ok(None);
        };
        let wasm = template
            .code
            .as_wasm_code()
            .ok_or(TemplateManagerError::UnsupportedTemplateType)?;
        let module = WasmModule::from_code(wasm);
        let loaded = module.load_template()?;

        Ok(Some(loaded))
    }

    fn has_template(&self, address: &TemplateAddress) -> Result<bool, Self::Error> {
        Ok(self.template_exists(address, Some(TemplateStatus::Active))? ||
            self.template_exists(address, Some(TemplateStatus::Deprecated))?)
    }
}

impl<TAddr> Clone for TemplateManager<TAddr> {
    fn clone(&self) -> Self {
        Self {
            global_db: self.global_db.clone(),
            config: self.config.clone(),
        }
    }
}

fn now() -> PrimitiveDateTime {
    let now = OffsetDateTime::now_utc();
    PrimitiveDateTime::new(now.date(), now.time())
}

fn convert_builtin_template(name: &str, address: TemplateAddress) -> Template {
    let code = get_template_builtin(&address);
    let binary_sha = calculate_template_binary_hash(code);
    Template {
        metadata: TemplateMetadata {
            name: name.to_string(),
            address,
            binary_sha,
            author_public_key: Default::default(),
            code_size: code.len(),
            epoch: Epoch::zero(),
        },
        code: TemplateCode::StaticWasm(code),
    }
}
