//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

use tari_common_types::types::PublicKey;
use tari_dan_common_types::services::template_provider::TemplateProvider;
use tari_dan_engine::{
    abi::TemplateDef,
    template::{LoadedTemplate, TemplateLoaderError, TemplateModuleLoader},
    wasm::{compile::compile_template, WasmModule},
};
use tari_engine_types::hashing::template_hasher32;
use tari_template_builtin::get_template_builtin;
use tari_template_lib::models::TemplateAddress;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Package {
    templates: Arc<Mutex<HashMap<TemplateAddress, LoadedTemplate>>>,
}

impl Package {
    pub fn builder() -> PackageBuilder {
        PackageBuilder::new()
    }

    pub fn get_template_by_address(&self, addr: &TemplateAddress) -> Option<LoadedTemplate> {
        self.templates.lock().unwrap().get(addr).cloned()
    }

    pub fn get_template_defs(&self) -> HashMap<TemplateAddress, TemplateDef> {
        self.templates
            .lock()
            .unwrap()
            .iter()
            .map(|(addr, template)| (*addr, template.template_def().clone()))
            .collect()
    }

    pub fn total_code_byte_size(&self) -> usize {
        self.templates.lock().unwrap().values().map(|t| t.code_size()).sum()
    }

    pub fn templates(&self) -> HashMap<TemplateAddress, LoadedTemplate> {
        self.templates.lock().unwrap().clone()
    }
}

#[derive(Debug, Clone, Default)]
pub struct PackageBuilder {
    templates: HashMap<TemplateAddress, LoadedTemplate>,
}

impl PackageBuilder {
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    pub fn add_template<P: AsRef<Path>>(&mut self, path: P) -> &mut Self {
        self.add_template_with_features(path, &[])
    }

    pub fn add_template_with_features<P: AsRef<Path>>(&mut self, path: P, features: &[&str]) -> &mut Self {
        let wasm = compile_template(path, features).unwrap();
        let template_addr = template_hasher32().chain(wasm.code()).result();
        let wasm = wasm.load_template().unwrap();
        self.add_loaded_template(template_addr, wasm);
        self
    }

    pub fn add_loaded_template(&mut self, address: TemplateAddress, template: LoadedTemplate) -> &mut Self {
        self.templates.insert(address, template);
        self
    }

    pub fn add_builtin_template(&mut self, address: &TemplateAddress) -> &mut Self {
        let wasm = get_template_builtin(address);
        let template = WasmModule::from_code(wasm.to_vec()).load_template().unwrap();
        self.add_loaded_template(*address, template);

        self
    }

    pub fn build(&mut self) -> Package {
        Package {
            templates: Arc::new(Mutex::new(self.templates.drain().collect())),
        }
    }
}

#[derive(Error, Debug)]
pub enum PackageError {
    #[error("Template load error: {0}")]
    TemplateLoad(#[from] TemplateLoaderError),
}
impl TemplateProvider for Package {
    type Error = PackageError;
    type Template = LoadedTemplate;

    fn get_template_module(
        &self,
        id: &tari_engine_types::TemplateAddress,
    ) -> Result<Option<Self::Template>, Self::Error> {
        Ok(self.templates.lock().unwrap().get(id).cloned())
    }

    fn add_wasm_template(
        &self,
        _author_public_key: PublicKey,
        template_address: tari_engine_types::TemplateAddress,
        template: &[u8],
    ) -> Result<(), Self::Error> {
        self.templates
            .lock()
            .unwrap()
            .insert(template_address, WasmModule::load_template_from_code(template)?);
        Ok(())
    }
}
