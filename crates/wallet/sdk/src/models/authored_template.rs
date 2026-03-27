// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_abi::{FunctionDef, TemplateDef, TemplateDefV1, version::WasmAbiVersion};
use tari_template_lib::types::{TemplateAddress, crypto::RistrettoPublicKeyBytes};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthoredTemplateModel {
    pub author_public_key: RistrettoPublicKeyBytes,
    pub address: TemplateAddress,
    pub name: String,
    pub abi_version: WasmAbiVersion,
    pub functions: Vec<FunctionDef>,
}

impl AuthoredTemplateModel {
    pub fn new(
        author_public_key: RistrettoPublicKeyBytes,
        template_address: TemplateAddress,
        template_def: TemplateDef,
    ) -> Self {
        Self {
            author_public_key,
            address: template_address,
            name: template_def.template_name().to_string(),
            abi_version: template_def.abi_version(),
            functions: template_def.functions().to_vec(),
        }
    }
}

impl From<AuthoredTemplateModel> for TemplateDef {
    fn from(model: AuthoredTemplateModel) -> Self {
        TemplateDef::V1(TemplateDefV1 {
            template_name: model.name,
            abi_version: model.abi_version,
            functions: model.functions,
        })
    }
}
