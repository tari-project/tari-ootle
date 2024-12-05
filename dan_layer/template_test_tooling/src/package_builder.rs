//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::RwLock;
use std::{collections::HashMap, convert::Infallible, path::Path};
use tari_common_types::types::PublicKey;
use tari_dan_common_types::services::template_provider::TemplateProvider;
use tari_dan_engine::{
    abi::TemplateDef,
    template::{LoadedTemplate, TemplateModuleLoader},
    wasm::{compile::compile_template, WasmModule},
};
use tari_engine_types::hashing::template_hasher32;
use tari_template_builtin::get_template_builtin;
use tari_template_lib::models::TemplateAddress;

#[derive(Debug, Clone)]
pub struct Package {
    templates: RwLock<HashMap<TemplateAddress, LoadedTemplate>>,
}

impl Package {
    pub fn builder() -> PackageBuilder {
        PackageBuilder::new()
    }

    pub fn get_template_by_address(&self, addr: &TemplateAddress) -> Option<&LoadedTemplate> {
        let lock = self.templates.read().unwrap();
        lock.get(addr)
    }

    pub fn get_template_defs(&self) -> HashMap<TemplateAddress, TemplateDef> {
        let lock = self.templates.read().unwrap();
        lock.iter()
            .map(|(addr, template)| (*addr, template.template_def().clone()))
            .collect()
    }

    pub fn total_code_byte_size(&self) -> usize {
        let lock = self.templates.read().unwrap();
        lock.values().map(|t| t.code_size()).sum()
    }

    pub fn iter(&self) -> impl Iterator<Item=(&TemplateAddress, &LoadedTemplate)> {
        let lock = self.templates.read().unwrap();
        lock.iter()
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
            templates: RwLock::new(self.templates.drain().collect()),
        }
    }
}

impl TemplateProvider for Package {
    type Error = Infallible;
    type Template = LoadedTemplate;

    fn get_template_module(
        &self,
        id: &tari_engine_types::TemplateAddress,
    ) -> Result<Option<Self::Template>, Self::Error> {
        let lock = self.templates.read()?;
        Ok(lock.get(id).cloned())
    }

    fn insert(&self, _author_public_key: PublicKey, template_address: tari_engine_types::TemplateAddress, template: &[u8]) -> Result<(), Self::Error> {
        let mut lock = self.templates.write()?;
        lock.insert(template_address, WasmModule::load_template_from_code(template)?);
        Ok(())
    }
}
