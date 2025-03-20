// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_engine_types::TemplateAddress;
use tari_template_abi::{FunctionDef, TemplateDef};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthoredTemplateModel {
    pub key_index: u64,
    pub address: TemplateAddress,
    pub name: String,
    pub tari_version: String,
    pub functions: Vec<FunctionDef>,
}

impl AuthoredTemplateModel {
    pub fn new(key_index: u64, template_address: TemplateAddress, template_def: TemplateDef) -> Self {
        Self {
            key_index,
            address: template_address,
            name: template_def.template_name().to_string(),
            tari_version: template_def.tari_version().to_string(),
            functions: template_def.functions().to_vec(),
        }
    }
}
