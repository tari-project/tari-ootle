//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, convert::Infallible, sync::Arc};

use tari_engine::template::LoadedTemplate;
use tari_ootle_common_types::services::template_provider::TemplateProvider;
use tari_template_lib::prelude::TemplateAddress;

#[derive(Debug, Clone)]
pub struct Package {
    templates: Arc<HashMap<TemplateAddress, LoadedTemplate>>,
}

impl Package {
    pub fn builder(capacity: usize) -> PackageBuilder {
        PackageBuilder {
            templates: HashMap::with_capacity(capacity),
        }
    }
}

impl TemplateProvider for Package {
    type Error = Infallible;
    type Template = LoadedTemplate;

    fn get_template(&self, address: &TemplateAddress) -> Result<Option<Self::Template>, Self::Error> {
        Ok(self.templates.get(address).cloned())
    }
}

pub struct PackageBuilder {
    templates: HashMap<TemplateAddress, LoadedTemplate>,
}

impl PackageBuilder {
    pub fn add_template(&mut self, address: TemplateAddress, template: LoadedTemplate) -> &mut Self {
        self.templates.insert(address, template);
        self
    }

    pub fn build(self) -> Package {
        Package {
            templates: Arc::new(self.templates),
        }
    }
}
