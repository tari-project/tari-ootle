//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    ffi::OsStr,
    path::Path,
    sync::{Arc, LazyLock, Mutex},
};

use tari_engine::{
    abi::TemplateDef,
    template::{LoadedTemplate, TemplateLoaderError, TemplateModuleLoader},
    wasm::WasmModule,
};
use tari_engine_types::hashing::hash_template_code;
use tari_ootle_common_types::services::template_provider::TemplateProvider;
use tari_template_builtin::all_builtin_templates;
use tari_template_lib::types::TemplateAddress;
use thiserror::Error;

use crate::compile::compile_template_with_envs;

static BUILTIN_TEMPLATES: LazyLock<Vec<(TemplateAddress, LoadedTemplate)>> = LazyLock::new(|| {
    all_builtin_templates()
        .iter()
        .map(|(addr, code)| {
            let template = WasmModule::from_code(*code)
                .load_template()
                .expect("failed to load builtin template");
            (*addr, template)
        })
        .collect()
});

fn cached_builtin_templates() -> &'static [(TemplateAddress, LoadedTemplate)] {
    &BUILTIN_TEMPLATES
}

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

    pub fn add_all_builtin_templates(&mut self) -> &mut Self {
        for (addr, template) in cached_builtin_templates() {
            self.add_loaded_template(*addr, template.clone());
        }
        self
    }

    pub fn add_template<P>(&mut self, path: P) -> TemplateAddress
    where P: AsRef<Path> {
        self.add_template_opts(path, &[], None::<(String, String)>)
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
        let wasm = compile_template_with_envs(path.as_ref(), features, envs)
            .unwrap_or_else(|e| panic!("Failed to compile template {}: {}", path.as_ref().display(), e));
        let template_addr = hash_template_code(wasm.code());
        let wasm = wasm.load_template().expect("failed to load template");
        self.add_loaded_template(template_addr, wasm);
        template_addr
    }

    pub fn add_loaded_template(&mut self, address: TemplateAddress, template: LoadedTemplate) -> &mut Self {
        self.templates.insert(address, template);
        self
    }

    pub fn add_template_from_code(
        &mut self,
        address: TemplateAddress,
        wasm: impl Into<Box<[u8]>>,
    ) -> Result<&mut Self, TemplateLoaderError> {
        let template = WasmModule::from_code(wasm).load_template()?;
        self.add_loaded_template(address, template);
        Ok(self)
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

    fn get_template(&self, id: &TemplateAddress) -> Result<Option<Self::Template>, Self::Error> {
        Ok(self.templates.lock().unwrap().get(id).cloned())
    }
}
