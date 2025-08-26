//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    ffi::OsStr,
    path::Path,
    sync::{Arc, Mutex},
};

use tari_engine::{
    abi::TemplateDef,
    template::{LoadedTemplate, TemplateLoaderError, TemplateModuleLoader},
    wasm::{compile::compile_template_with_envs, WasmModule},
};
use tari_engine_types::hashing::hash_template_code;
use tari_ootle_common_types::services::template_provider::TemplateProvider;
use tari_template_builtin::get_template_builtin;
use tari_template_lib::types::TemplateAddress;
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

    pub fn add_template<P>(&mut self, path: P) -> &mut Self
    where P: AsRef<Path> {
        self.add_template_opts(path, &[], None::<(String, String)>);
        self
    }

    pub fn add_template_with_envs<P, TEnvs, K, V>(&mut self, path: P, envs: TEnvs) -> &mut Self
    where
        P: AsRef<Path>,
        TEnvs: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.add_template_opts(path, &[], envs);
        self
    }

    pub fn add_template_opts<P, TEnvs, K, V>(&mut self, path: P, features: &[&str], envs: TEnvs) -> TemplateAddress
    where
        P: AsRef<Path>,
        TEnvs: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let wasm = compile_template_with_envs(path, features, envs).unwrap();
        let template_addr = hash_template_code(wasm.code());
        let wasm = wasm.load_template().unwrap();
        self.add_loaded_template(template_addr, wasm);
        template_addr
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

    fn get_template_module(&self, id: &TemplateAddress) -> Result<Option<Self::Template>, Self::Error> {
        Ok(self.templates.lock().unwrap().get(id).cloned())
    }
}
